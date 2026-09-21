//! Bounded, cited web-evidence review for the daily BLUE showcase.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use store::{BlueEvidenceClaim, BlueEvidenceResult, BlueEvidenceSourceInput};
use unicode_normalization::{UnicodeNormalization as _, char::is_combining_mark};

use crate::blue_feux_de_foret::FeuxDeForetClient;

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

#[derive(Clone)]
pub struct BlueEvidenceReviewer {
    client: Client,
    api_key: String,
    model: String,
    feux_de_foret: Option<FeuxDeForetClient>,
}

impl BlueEvidenceReviewer {
    pub fn new(
        api_key: String,
        model: String,
        feux_de_foret_enabled: bool,
    ) -> Result<Self, BlueEvidenceError> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_mins(2))
            .build()?;
        Ok(Self {
            client,
            api_key,
            model,
            feux_de_foret: feux_de_foret_enabled
                .then(FeuxDeForetClient::new)
                .transpose()?,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn request_checksum(&self, claim: &BlueEvidenceClaim) -> String {
        let payload = self.request_payload(claim);
        format!("{:x}", Sha256::digest(payload.to_string().as_bytes()))
    }

    pub async fn review(
        &self,
        claim: &BlueEvidenceClaim,
    ) -> Result<BlueEvidenceResult, BlueEvidenceError> {
        match self.review_feux_de_foret(claim).await {
            Ok(Some(result)) => return Ok(result),
            Ok(None) => {}
            Err(error) => tracing::warn!(
                %error,
                commune = %claim.commune_name,
                "direct terrain lookup failed; falling back to bounded OpenAI web search"
            ),
        }
        let response = self
            .client
            .post(RESPONSES_URL)
            .bearer_auth(&self.api_key)
            .json(&self.request_payload(claim))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(BlueEvidenceError::Api(format!(
                "OpenAI Responses API returned {status}: {}",
                body.chars().take(300).collect::<String>()
            )));
        }
        let mut result = parse_response(&body)?;
        validate_evidence_result(&mut result, claim, Utc::now())?;
        Ok(result)
    }

    async fn review_feux_de_foret(
        &self,
        claim: &BlueEvidenceClaim,
    ) -> Result<Option<BlueEvidenceResult>, BlueEvidenceError> {
        let Some(client) = &self.feux_de_foret else {
            return Ok(None);
        };
        let window_end = match claim.review_horizon.as_str() {
            "hours_24" => claim.alert_24h_valid_at,
            "hours_48" => claim.alert_48h_valid_at,
            _ => None,
        };
        let Some(window_end) = window_end else {
            return Ok(None);
        };
        let Some(report) = client
            .find_report(
                &claim.commune_name,
                claim.department_code.as_deref(),
                claim.issued_at,
                window_end,
            )
            .await?
        else {
            return Ok(None);
        };
        let source = BlueEvidenceSourceInput {
            url: report.url.clone(),
            title: report.title.clone(),
            published_at: Some(report.occurred_at),
            excerpt: Some(format!(
                "Signal communautaire daté concernant {}. À corroborer par une source officielle.",
                claim.commune_name
            )),
            domain: "feuxdeforet.fr".to_owned(),
            relation_strength: "direct".to_owned(),
        };
        let mut result = BlueEvidenceResult {
            verdict: "probable".to_owned(),
            confidence: 0.78,
            summary: format!(
                "Un signal d'incendie daté correspond à {} dans la fenêtre BLUE. La source est communautaire : le résultat reste probable jusqu'à corroboration officielle.",
                claim.commune_name
            ),
            observed_event_at: Some(report.occurred_at),
            observed_location: Some(claim.commune_name.clone()),
            response_id: format!("feuxdeforet:{}", report.id),
            input_tokens: None,
            output_tokens: None,
            web_search_count: 1,
            sources: vec![source],
            raw_response: json!({
                "provider": "feuxdeforet",
                "report_id": report.id,
                "output": [{
                    "type": "web_search_call",
                    "action": {"sources": [{"url": report.url}]}
                }]
            }),
        };
        validate_evidence_result(&mut result, claim, Utc::now())?;
        Ok(Some(result))
    }

    fn request_payload(&self, claim: &BlueEvidenceClaim) -> Value {
        let case_data = json!({
            "bulletin_date": claim.bulletin_date,
            "issued_at": claim.issued_at,
            "commune": claim.commune_name,
            "insee_code": claim.insee_code,
            "department_code": claim.department_code,
            "blue_daily_rank": claim.daily_rank,
            "blue_selection_score": claim.selection_score,
            "selection_reason": claim.selection_reason,
            "trigger_observed_at": claim.trigger_observed_at,
            "review_horizon": claim.review_horizon,
            "forecast_24h": {
                "index": claim.alert_24h_index,
                "valid_at": claim.alert_24h_valid_at,
            },
            "forecast_48h": {
                "index": claim.alert_48h_index,
                "valid_at": claim.alert_48h_valid_at,
            }
        });
        json!({
            "model": self.model,
            "tools": [{"type": "web_search"}],
            "include": ["web_search_call.action.sources"],
            "instructions": "Tu es le vérificateur de preuves de BLUE. Tu dois obligatoirement utiliser web_search avant tout verdict positif. Commence notamment par une recherche ciblée site:feuxdeforet.fr, puis cherche une corroboration auprès des autorités, secours et de la presse locale établie. Recherche uniquement des traces d'incendie ou de feu de végétation dans la commune exacte et dans la fenêtre exacte qui commence à issued_at et se termine au valid_at du review_horizon demandé. Inclue le nom exact de la commune, le département, l'année et les dates de la fenêtre dans les requêtes. Ignore tout événement ancien, même célèbre, toute commune voisine et toute source qui ne date pas explicitement l'événement. Une page intitulée fausse alerte doit toujours être rejetée. FeuxDeForet est une source communautaire qui peut au maximum produire probable, jamais confirmed. Lorsqu'un trigger_observed_at est fourni, il s'agit d'une recherche réactive déclenchée par un signal thermique: cherche immédiatement une confirmation indépendante sans jamais considérer le signal seul comme un incendie. Pour hours_24, produis un constat provisoire limité aux premières 24 heures. Pour hours_48, produis le constat final couvrant les 48 heures complètes, même si une première recherche a déjà eu lieu. Ne considère jamais une absence de résultat comme la preuve qu'aucun incendie n'a eu lieu. N'invente aucune source ni URL. Chaque URL du tableau evidence doit provenir directement des résultats de web_search. Un verdict confirmed exige une source officielle directe et une concordance exacte de commune et de date. Une source de presse crédible peut au maximum produire probable. Une source inconnue, une date absente ou une localisation approximative impose inconclusive ou no_evidence_found. Tous les champs de date doivent être soit null, soit au format RFC3339 complet avec fuseau horaire, par exemple 2026-08-16T01:30:00+02:00. N'utilise jamais une date en langage naturel. Réponds en français, sobrement.",
            "input": [{
                "role": "user",
                "content": format!("Vérifie ce dossier de prévision BLUE après échéance. Données du dossier: {case_data}")
            }],
            "text": {"format": evidence_schema()}
        })
    }
}

fn evidence_schema() -> Value {
    json!({
        "type": "json_schema",
        "name": "blue_evidence_review",
        "strict": true,
        "schema": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "verdict": {"type": "string", "enum": ["signal_observed","probable","confirmed","no_evidence_found","inconclusive"]},
                "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                "summary": {"type": "string", "maxLength": 1200},
                "observed_event_at": {"type": ["string","null"], "description": "Date RFC3339 complete avec fuseau horaire, ou null"},
                "observed_location": {"type": ["string","null"], "maxLength": 300},
                "evidence": {
                    "type": "array", "maxItems": 8,
                    "items": {
                        "type": "object", "additionalProperties": false,
                        "properties": {
                            "url": {"type": "string"},
                            "title": {"type": "string", "maxLength": 300},
                            "published_at": {"type": ["string","null"], "description": "Date RFC3339 complete avec fuseau horaire, ou null"},
                            "excerpt": {"type": ["string","null"], "maxLength": 500},
                            "relation_strength": {"type": "string", "enum": ["direct","corroborating","weak"]}
                        },
                        "required": ["url","title","published_at","excerpt","relation_strength"]
                    }
                }
            },
            "required": ["verdict","confidence","summary","observed_event_at","observed_location","evidence"]
        }
    })
}

#[derive(Deserialize)]
struct GeneratedReview {
    verdict: String,
    confidence: f32,
    summary: String,
    observed_event_at: Option<String>,
    observed_location: Option<String>,
    evidence: Vec<GeneratedEvidence>,
}

#[derive(Deserialize)]
struct GeneratedEvidence {
    url: String,
    title: String,
    published_at: Option<String>,
    excerpt: Option<String>,
    relation_strength: String,
}

fn parse_response(body: &str) -> Result<BlueEvidenceResult, BlueEvidenceError> {
    let raw: Value = serde_json::from_str(body)?;
    let text = raw
        .get("output")
        .and_then(Value::as_array)
        .and_then(|output| {
            output.iter().find_map(|item| {
                if item.get("type").and_then(Value::as_str) != Some("message") {
                    return None;
                }
                item.get("content")
                    .and_then(Value::as_array)
                    .and_then(|content| {
                        content.iter().find_map(|part| {
                            (part.get("type").and_then(Value::as_str) == Some("output_text"))
                                .then(|| part.get("text").and_then(Value::as_str))
                                .flatten()
                        })
                    })
            })
        })
        .ok_or(BlueEvidenceError::MissingOutput)?;
    let generated: GeneratedReview = serde_json::from_str(text)?;
    let mut sources = generated
        .evidence
        .into_iter()
        .filter_map(|item| normalize_source(item).ok())
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| left.url.cmp(&right.url));
    sources.dedup_by(|left, right| left.url == right.url);

    let has_direct = sources
        .iter()
        .any(|source| source.relation_strength == "direct");
    let verdict = match generated.verdict.as_str() {
        "confirmed" if has_direct => "confirmed",
        "confirmed" if !sources.is_empty() => "probable",
        "probable" | "signal_observed" if sources.is_empty() => "inconclusive",
        value => value,
    }
    .to_owned();
    let observed_event_at = parse_optional_datetime(generated.observed_event_at.as_deref());
    let web_search_count = raw
        .get("output")
        .and_then(Value::as_array)
        .map_or(0, |output| {
            i64::try_from(
                output
                    .iter()
                    .filter(|item| {
                        item.get("type").and_then(Value::as_str) == Some("web_search_call")
                    })
                    .count(),
            )
            .unwrap_or(i64::MAX)
        });
    Ok(BlueEvidenceResult {
        verdict,
        confidence: generated.confidence.clamp(0.0, 1.0),
        summary: generated.summary.chars().take(1_200).collect(),
        observed_event_at,
        observed_location: generated
            .observed_location
            .map(|value| value.chars().take(300).collect()),
        response_id: raw
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        input_tokens: raw.pointer("/usage/input_tokens").and_then(Value::as_i64),
        output_tokens: raw.pointer("/usage/output_tokens").and_then(Value::as_i64),
        web_search_count,
        sources,
        raw_response: raw,
    })
}

fn normalize_source(
    source: GeneratedEvidence,
) -> Result<BlueEvidenceSourceInput, BlueEvidenceError> {
    let parsed = Url::parse(&source.url)
        .map_err(|_| BlueEvidenceError::InvalidOutput("invalid evidence URL".to_owned()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(BlueEvidenceError::InvalidOutput(
            "unsupported evidence URL scheme".to_owned(),
        ));
    }
    let domain = parsed
        .host_str()
        .ok_or_else(|| BlueEvidenceError::InvalidOutput("evidence URL has no host".to_owned()))?
        .to_owned();
    let published_at = parse_optional_datetime(source.published_at.as_deref());
    Ok(BlueEvidenceSourceInput {
        url: parsed.to_string().chars().take(2_000).collect(),
        title: source.title.chars().take(300).collect(),
        published_at,
        excerpt: source
            .excerpt
            .map(|value| value.chars().take(500).collect()),
        domain,
        relation_strength: source.relation_strength,
    })
}

fn parse_optional_datetime(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

#[allow(clippy::too_many_lines)]
fn validate_evidence_result(
    result: &mut BlueEvidenceResult,
    claim: &BlueEvidenceClaim,
    checked_at: DateTime<Utc>,
) -> Result<(), BlueEvidenceError> {
    if !matches!(
        result.verdict.as_str(),
        "confirmed" | "probable" | "signal_observed"
    ) {
        result.sources.clear();
        result.observed_event_at = None;
        result.observed_location = None;
        result.confidence = result.confidence.min(0.50);
        result.summary = if result.verdict == "no_evidence_found" {
            format!(
                "Aucune preuve fiable et datée n'a été trouvée pour {} dans la fenêtre contrôlée. Cette absence de preuve ne démontre pas une absence d'incendie.",
                claim.commune_name
            )
        } else {
            format!(
                "La recherche concernant {} reste non concluante après application des contrôles de date, de lieu et de source.",
                claim.commune_name
            )
        };
        return Ok(());
    }
    if result.web_search_count == 0 {
        return Err(invalid_output("positive verdict without web search"));
    }
    let window_end = match claim.review_horizon.as_str() {
        "hours_24" => claim.alert_24h_valid_at,
        "hours_48" => claim.alert_48h_valid_at,
        value => {
            return Err(invalid_output(format!(
                "unsupported review horizon {value}"
            )));
        }
    }
    .ok_or_else(|| invalid_output("positive verdict without forecast window"))?;
    let event_at = result
        .observed_event_at
        .ok_or_else(|| invalid_output("positive verdict without a valid event date"))?;
    if event_at < claim.issued_at || event_at > window_end {
        return Err(invalid_output("event date outside the forecast window"));
    }
    let observed_location = result
        .observed_location
        .as_deref()
        .ok_or_else(|| invalid_output("positive verdict without an observed location"))?;
    if !contains_normalized(observed_location, &claim.commune_name) {
        return Err(invalid_output(
            "observed location does not match the commune",
        ));
    }

    let grounded_urls = grounded_response_urls(&result.raw_response);
    if grounded_urls.is_empty() {
        return Err(invalid_output("web search returned no citable URL"));
    }
    let latest_publication = checked_at + Duration::minutes(5);
    let mut has_authority = false;
    let mut has_established_press = false;
    let mut has_direct_authority = false;
    for source in &result.sources {
        let canonical = canonical_url(&source.url)
            .ok_or_else(|| invalid_output("evidence URL cannot be canonicalized"))?;
        if !grounded_urls.contains(&canonical) {
            return Err(invalid_output(
                "evidence URL was not returned by web search",
            ));
        }
        let published_at = source
            .published_at
            .ok_or_else(|| invalid_output("positive evidence has no publication date"))?;
        if published_at < claim.issued_at || published_at > latest_publication {
            return Err(invalid_output(
                "evidence publication date is outside the review period",
            ));
        }
        let searchable_text = format!(
            "{} {}",
            source.title,
            source.excerpt.as_deref().unwrap_or_default()
        );
        if !contains_normalized(&searchable_text, &claim.commune_name) {
            return Err(invalid_output("evidence text does not name the commune"));
        }
        if is_authority_domain(&source.domain) {
            has_authority = true;
            has_direct_authority |= source.relation_strength == "direct";
        } else if is_established_press_domain(&source.domain)
            || is_community_signal_domain(&source.domain)
        {
            has_established_press = true;
        } else {
            return Err(invalid_output("evidence domain is not trusted"));
        }
    }
    if result.sources.is_empty() || (!has_authority && !has_established_press) {
        return Err(invalid_output("positive verdict without trusted evidence"));
    }
    if result.verdict == "confirmed" && !has_direct_authority {
        "probable".clone_into(&mut result.verdict);
        result.confidence = result.confidence.min(0.85);
    } else if result.verdict == "confirmed" {
        result.confidence = result.confidence.min(0.95);
    } else if result.verdict == "probable" {
        result.confidence = result.confidence.min(0.85);
    } else {
        result.confidence = result.confidence.min(0.70);
    }
    Ok(())
}

fn invalid_output(message: impl Into<String>) -> BlueEvidenceError {
    BlueEvidenceError::InvalidOutput(message.into())
}

fn grounded_response_urls(raw: &Value) -> HashSet<String> {
    let mut urls = HashSet::new();
    let Some(output) = raw.get("output").and_then(Value::as_array) else {
        return urls;
    };
    for item in output {
        if let Some(sources) = item.pointer("/action/sources").and_then(Value::as_array) {
            for source in sources {
                if let Some(url) = source.get("url").and_then(Value::as_str)
                    && let Some(canonical) = canonical_url(url)
                {
                    urls.insert(canonical);
                }
            }
        }
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in content {
            let Some(annotations) = part.get("annotations").and_then(Value::as_array) else {
                continue;
            };
            for annotation in annotations {
                if let Some(url) = annotation.get("url").and_then(Value::as_str)
                    && let Some(canonical) = canonical_url(url)
                {
                    urls.insert(canonical);
                }
            }
        }
    }
    urls
}

fn canonical_url(value: &str) -> Option<String> {
    let mut url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string().trim_end_matches('/').to_owned())
}

fn contains_normalized(haystack: &str, needle: &str) -> bool {
    let haystack = format!(" {} ", normalize_text(haystack));
    let needle = normalize_text(needle);
    !needle.is_empty() && haystack.contains(&format!(" {needle} "))
}

fn normalize_text(value: &str) -> String {
    value
        .nfd()
        .filter(|character| !is_combining_mark(*character))
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_authority_domain(domain: &str) -> bool {
    let domain = domain.to_ascii_lowercase();
    domain.ends_with(".gouv.fr")
        || domain == "gouv.fr"
        || domain.ends_with(".santepubliquefrance.fr")
        || domain == "santepubliquefrance.fr"
        || domain.ends_with(".onf.fr")
        || domain == "onf.fr"
        || domain.split('.').any(|label| label.starts_with("sdis"))
        || domain.split('.').any(|label| label.starts_with("pompiers"))
}

fn is_established_press_domain(domain: &str) -> bool {
    const PRESS_DOMAINS: &[&str] = &[
        "20minutes.fr",
        "actu.fr",
        "bfmtv.com",
        "dna.fr",
        "estrepublicain.fr",
        "francebleu.fr",
        "francetvinfo.fr",
        "ici.fr",
        "ladepeche.fr",
        "lardennais.fr",
        "ledauphine.com",
        "lefigaro.fr",
        "lindependant.fr",
        "lemonde.fr",
        "midilibre.fr",
        "ouest-france.fr",
        "republicain-lorrain.fr",
        "sudouest.fr",
        "tf1info.fr",
    ];
    let domain = domain.to_ascii_lowercase();
    PRESS_DOMAINS
        .iter()
        .any(|trusted| domain == *trusted || domain.ends_with(&format!(".{trusted}")))
}

fn is_community_signal_domain(domain: &str) -> bool {
    let domain = domain.to_ascii_lowercase();
    domain == "feuxdeforet.fr" || domain.ends_with(".feuxdeforet.fr")
}

#[derive(Debug, thiserror::Error)]
pub enum BlueEvidenceError {
    #[error("HTTP client error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("direct terrain source error: {0}")]
    FeuxDeForet(#[from] crate::blue_feux_de_foret::FeuxDeForetError),
    #[error("invalid JSON response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("OpenAI API error: {0}")]
    Api(String),
    #[error("OpenAI response contained no structured output")]
    MissingOutput,
    #[error("invalid evidence output: {0}")]
    InvalidOutput(String),
}

impl BlueEvidenceError {
    /// Whether the provider explicitly rejected the request because the
    /// configured account has no remaining credit. This is an availability
    /// condition, not a failed evidence search, so the scheduler must avoid
    /// consuming the bounded review attempts for every queued commune.
    pub fn is_quota_exhausted(&self) -> bool {
        matches!(self, Self::Api(message) if {
            let message = message.to_ascii_lowercase();
            message.contains("insufficient_quota") || message.contains("no credits remaining")
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};
    use store::BlueEvidenceClaim;

    use super::{BlueEvidenceError, parse_response, validate_evidence_result};

    #[test]
    fn identifies_explicit_provider_credit_exhaustion() {
        let exhausted = BlueEvidenceError::Api(
            "Responses API returned 429: insufficient_quota; no credits remaining".to_owned(),
        );
        let ordinary_rate_limit =
            BlueEvidenceError::Api("Responses API returned 429: please retry later".to_owned());

        assert!(exhausted.is_quota_exhausted());
        assert!(!ordinary_rate_limit.is_quota_exhausted());
    }

    fn claim(commune_name: &str) -> BlueEvidenceClaim {
        BlueEvidenceClaim {
            id: "00000000-0000-0000-0000-000000000001".to_owned(),
            bulletin_id: "00000000-0000-0000-0000-000000000002".to_owned(),
            bulletin_date: Utc
                .with_ymd_and_hms(2026, 8, 16, 0, 0, 0)
                .single()
                .expect("date")
                .date_naive(),
            issued_at: Utc
                .with_ymd_and_hms(2026, 8, 16, 6, 40, 0)
                .single()
                .expect("issued at"),
            insee_code: "07000".to_owned(),
            commune_name: commune_name.to_owned(),
            department_code: Some("07".to_owned()),
            daily_rank: 1,
            selection_score: 0.95,
            selection_reason: "national_top".to_owned(),
            trigger_observed_at: None,
            alert_24h_index: Some(0.95),
            alert_24h_valid_at: Utc.with_ymd_and_hms(2026, 8, 17, 6, 0, 0).single(),
            alert_48h_index: Some(0.90),
            alert_48h_valid_at: Utc.with_ymd_and_hms(2026, 8, 18, 6, 0, 0).single(),
            review_horizon: "hours_24".to_owned(),
            attempt_count: 1,
            stage_attempt_count: 1,
        }
    }

    fn checked_at() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 17, 9, 0, 0)
            .single()
            .expect("checked at")
    }

    fn grounded_response(
        commune: &str,
        domain_url: &str,
        verdict: &str,
        event_at: &str,
        published_at: &str,
        location: &str,
    ) -> String {
        serde_json::json!({
            "id":"resp_grounded",
            "output":[
                {"type":"web_search_call","action":{"type":"search","sources":[
                    {"type":"url","url":domain_url}
                ]}},
                {"type":"message","content":[{
                    "type":"output_text","annotations":[{"type":"url_citation","url":domain_url}],
                    "text":serde_json::json!({
                        "verdict":verdict,"confidence":0.92,
                        "summary":format!("Incendie documenté à {commune}."),
                        "observed_event_at":event_at,"observed_location":location,
                        "evidence":[{"url":domain_url,"title":format!("Incendie à {commune}"),
                            "published_at":published_at,
                            "excerpt":format!("Les secours interviennent à {commune}."),
                            "relation_strength":"direct"}]
                    }).to_string()
                }]}
            ]
        })
        .to_string()
    }

    #[test]
    fn downgrades_confirmation_without_direct_evidence() {
        let body = serde_json::json!({
            "id":"resp_test","usage":{"input_tokens":12,"output_tokens":8},
            "output":[{"type":"web_search_call"},{"type":"message","content":[{
                "type":"output_text","text":serde_json::json!({
                    "verdict":"confirmed","confidence":0.9,"summary":"Signal local.",
                    "observed_event_at":null,"observed_location":"Commune test",
                    "evidence":[{"url":"https://example.org/fire","title":"Feu local",
                        "published_at":null,"excerpt":"Un signal.","relation_strength":"weak"}]
                }).to_string()
            }]}]
        })
        .to_string();
        let result = parse_response(&body).expect("valid response");
        assert_eq!(result.verdict, "probable");
        assert_eq!(result.web_search_count, 1);
        assert_eq!(result.sources.len(), 1);
    }

    #[test]
    fn evidence_free_probability_is_inconclusive() {
        let body = serde_json::json!({
            "id":"resp_test","output":[{"type":"message","content":[{
                "type":"output_text","text":serde_json::json!({
                    "verdict":"probable","confidence":0.4,"summary":"Sans source.",
                    "observed_event_at":null,"observed_location":null,"evidence":[]
                }).to_string()
            }]}]
        })
        .to_string();
        let result = parse_response(&body).expect("valid response");
        assert_eq!(result.verdict, "inconclusive");
    }

    #[test]
    fn malformed_optional_dates_do_not_discard_valid_evidence() {
        let body = serde_json::json!({
            "id":"resp_date_repair","output":[{"type":"message","content":[{
                "type":"output_text","text":serde_json::json!({
                    "verdict":"confirmed","confidence":0.92,"summary":"Feu confirme.",
                    "observed_event_at":"dans la nuit du 15 au 16 aout",
                    "observed_location":"Montherme",
                    "evidence":[{"url":"https://example.org/montherme","title":"Feu local",
                        "published_at":"16 aout 2026","excerpt":"Intervention nocturne.",
                        "relation_strength":"direct"}]
                }).to_string()
            }]}]
        })
        .to_string();
        let result = parse_response(&body).expect("optional malformed dates are omitted");
        assert_eq!(result.verdict, "confirmed");
        assert_eq!(result.observed_event_at, None);
        assert_eq!(result.sources[0].published_at, None);
    }

    #[test]
    fn rejects_positive_verdict_without_a_real_web_search() {
        let body = serde_json::json!({
            "id":"resp_hallucinated","output":[{"type":"message","content":[{
                "type":"output_text","text":serde_json::json!({
                    "verdict":"probable","confidence":0.7,"summary":"Incendie à Ucel.",
                    "observed_event_at":"2026-08-16T12:00:00Z","observed_location":"Ucel, 07",
                    "evidence":[{"url":"https://local-news.fr/incendie-foret-ucel",
                        "title":"Incendie à Ucel","published_at":"2026-08-16T14:00:00Z",
                        "excerpt":"Feu à Ucel.","relation_strength":"corroborating"}]
                }).to_string()
            }]}]
        })
        .to_string();
        let mut result = parse_response(&body).expect("structured response");
        let error = validate_evidence_result(&mut result, &claim("Ucel"), checked_at())
            .expect_err("ungrounded positive must be rejected");
        assert!(error.to_string().contains("without web search"));
    }

    #[test]
    fn removes_irrelevant_sources_from_a_no_evidence_result() {
        let body = serde_json::json!({
            "id":"resp_negative","output":[{"type":"message","content":[{
                "type":"output_text","text":serde_json::json!({
                    "verdict":"no_evidence_found","confidence":0.8,
                    "summary":"Un ancien feu a été trouvé ailleurs.",
                    "observed_event_at":"2025-08-16T12:00:00Z","observed_location":"Autre ville",
                    "evidence":[{"url":"https://example.org/ancien-feu",
                        "title":"Ancien feu","published_at":"2025-08-16T14:00:00Z",
                        "excerpt":"Ancien événement.","relation_strength":"weak"}]
                }).to_string()
            }]}]
        })
        .to_string();
        let mut result = parse_response(&body).expect("structured response");
        validate_evidence_result(&mut result, &claim("Ucel"), checked_at())
            .expect("negative result is safely normalized");
        assert!(result.sources.is_empty());
        assert_eq!(result.observed_event_at, None);
        assert_eq!(result.observed_location, None);
        assert!(result.summary.contains("Aucune preuve fiable"));
        assert!(result.confidence <= 0.5);
    }

    #[test]
    fn rejects_an_old_event_even_when_the_source_was_searched() {
        let body = grounded_response(
            "Narbonne",
            "https://www.aude.gouv.fr/incendie-ribaute",
            "confirmed",
            "2025-08-05T14:15:00Z",
            "2025-08-07T10:00:00Z",
            "Narbonne, Aude",
        );
        let mut result = parse_response(&body).expect("structured response");
        let error = validate_evidence_result(&mut result, &claim("Narbonne"), checked_at())
            .expect_err("old event must be rejected");
        assert!(error.to_string().contains("outside the forecast window"));
    }

    #[test]
    fn rejects_a_neighboring_commune() {
        let body = grounded_response(
            "Ribaute",
            "https://www.aude.gouv.fr/incendie-ribaute-2026",
            "confirmed",
            "2026-08-16T14:15:00Z",
            "2026-08-16T16:00:00Z",
            "Ribaute, Aude",
        );
        let mut result = parse_response(&body).expect("structured response");
        let error = validate_evidence_result(&mut result, &claim("Carcassonne"), checked_at())
            .expect_err("neighboring commune must be rejected");
        assert!(error.to_string().contains("does not match the commune"));
    }

    #[test]
    fn accepts_a_grounded_official_confirmation_in_the_exact_window() {
        let body = grounded_response(
            "Ucel",
            "https://www.ardeche.gouv.fr/incendie-ucel-2026",
            "confirmed",
            "2026-08-16T12:00:00Z",
            "2026-08-16T14:00:00Z",
            "Ucel, Ardèche",
        );
        let mut result = parse_response(&body).expect("structured response");
        validate_evidence_result(&mut result, &claim("Ucel"), checked_at())
            .expect("official exact evidence");
        assert_eq!(result.verdict, "confirmed");
    }

    #[test]
    fn press_evidence_can_never_be_automatically_confirmed() {
        let body = grounded_response(
            "Ucel",
            "https://www.ledauphine.com/incendie-ucel-2026",
            "confirmed",
            "2026-08-16T12:00:00Z",
            "2026-08-16T14:00:00Z",
            "Ucel, Ardèche",
        );
        let mut result = parse_response(&body).expect("structured response");
        validate_evidence_result(&mut result, &claim("Ucel"), checked_at())
            .expect("established press evidence");
        assert_eq!(result.verdict, "probable");
        assert!(result.confidence <= 0.85);
    }
}
