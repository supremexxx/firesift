use std::{net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use grid::BoundingBox;

const DEFAULT_DATABASE_URL: &str = "postgres://pyrorisk:pyrorisk@localhost:5432/pyrorisk";
const DEFAULT_DATA_PROFILE: &str = "fixture";
const DEFAULT_FIRMS_FIXTURE_PATH: &str = "testdata/firms_viirs_snpp.csv";
const DEFAULT_METEOFRANCE_FIXTURE_PATH: &str = "testdata/meteo_france_synop.csv";
const DEFAULT_BACKTEST_WEATHER_PATH: &str = "data/synop";
const DEFAULT_WEATHER_IDW_POWER: &str = "2.0";
const DEFAULT_WEATHER_CACHE_DIR: &str = "out/weather";
const DEFAULT_OSM_PATH: &str = "testdata/osm_features.csv";
const DEFAULT_BDIFF_PATH: &str = "testdata/bdiff_aude.csv";
const DEFAULT_PROMETHEE_PATH: &str = "testdata/promethee_aude.csv";
const DEFAULT_CORINE_PATH: &str = "testdata/corine_aude.csv";
const DEFAULT_INSEE_PATH: &str = "testdata/insee_filosofi_200m.csv";
const DEFAULT_CALENDAR_PATH: &str = "testdata/calendar_zone_c.csv";
const DEFAULT_AOI_BBOX: &str = "1.68,42.57,3.26,43.46";
const DEFAULT_H3_RESOLUTION: &str = "9";
const DEFAULT_RECOMPUTE_INTERVAL_SECS: &str = "900";
const DEFAULT_FWI_MAX: &str = "30.0";
const DEFAULT_RISK_ALPHA: &str = "0.6";
const DEFAULT_RISK_BETA: &str = "0.4";
const DEFAULT_RISK_W_HIST: &str = "0.4";
const DEFAULT_RISK_W_WUI: &str = "0.25";
const DEFAULT_RISK_W_ROAD: &str = "0.2";
const DEFAULT_RISK_W_AGRI: &str = "0.15";
const DEFAULT_API_BIND: &str = "0.0.0.0:8080";
/// Directory of the built web frontend (`web/dist` after `npm run build`
/// in `web/`) served as static files, with an `index.html` fallback for
/// client-side routing. Defaults to a path relative to the working
/// directory for local development; the container image sets this to an
/// absolute path (see `Dockerfile`). Missing this directory is not an
/// error -- requests for it simply 404, so the API works standalone
/// without building the frontend first.
const DEFAULT_WEB_ASSETS_DIR: &str = "web/dist";
/// Gates the scheduler's BLUE evidence-archiving tasks (`poll_blue_evidence`,
/// the daily bulletin capture in `poll_forecast`) -- data collection, not
/// an HTTP interface. All bundled web interfaces (including BLUE's) were
/// removed on 2026-08-30 to be rebuilt from scratch, but this flag is kept
/// so the evidence archive keeps building in the meantime; see
/// `ROADMAP.md` for the removal record.
const DEFAULT_BLUE_CENTER_ENABLED: &str = "false";
const DEFAULT_BLUE_AI_EVIDENCE_ENABLED: &str = "false";
const DEFAULT_BLUE_FEUX_DE_FORET_ENABLED: &str = "false";
const DEFAULT_BLUE_OPENAI_MODEL: &str = "gpt-4o-mini";

#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct Config {
    pub database_url: String,
    pub data_profile: DataProfile,
    pub firms_map_key: Option<String>,
    pub firms_fixture_path: PathBuf,
    pub meteofrance_api_key: Option<String>,
    pub meteofrance_fixture_path: PathBuf,
    pub backtest_weather_path: PathBuf,
    pub weather_idw_power: f64,
    pub weather_cache_dir: PathBuf,
    pub osm_path: PathBuf,
    pub bdiff_path: PathBuf,
    pub promethee_path: PathBuf,
    pub corine_path: PathBuf,
    pub insee_path: PathBuf,
    pub calendar_path: PathBuf,
    pub territory_geojson_path: Option<PathBuf>,
    pub territory_codes: Vec<String>,
    pub territory_label: Option<String>,
    pub aoi_bbox: BoundingBox,
    pub h3_resolution: u8,
    pub recompute_interval: Duration,
    pub risk: RiskConfig,
    pub api_bind: SocketAddr,
    pub web_assets_dir: PathBuf,
    pub blue_center_enabled: bool,
    pub blue_ai_evidence_enabled: bool,
    pub blue_feux_de_foret_enabled: bool,
    pub openai_api_key: Option<String>,
    pub blue_openai_model: String,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let config = Self {
            database_url: env_or_default("DATABASE_URL", DEFAULT_DATABASE_URL),
            data_profile: parse_env("DATA_PROFILE", DEFAULT_DATA_PROFILE)?,
            firms_map_key: optional_env("FIRMS_MAP_KEY"),
            firms_fixture_path: PathBuf::from(env_or_default(
                "FIRMS_FIXTURE_PATH",
                DEFAULT_FIRMS_FIXTURE_PATH,
            )),
            meteofrance_api_key: optional_env("METEOFRANCE_API_KEY"),
            meteofrance_fixture_path: PathBuf::from(env_or_default(
                "METEOFRANCE_FIXTURE_PATH",
                DEFAULT_METEOFRANCE_FIXTURE_PATH,
            )),
            backtest_weather_path: PathBuf::from(env_or_default(
                "BACKTEST_WEATHER_PATH",
                DEFAULT_BACKTEST_WEATHER_PATH,
            )),
            weather_idw_power: parse_env("WEATHER_IDW_POWER", DEFAULT_WEATHER_IDW_POWER)?,
            weather_cache_dir: PathBuf::from(env_or_default(
                "WEATHER_CACHE_DIR",
                DEFAULT_WEATHER_CACHE_DIR,
            )),
            osm_path: PathBuf::from(env_or_default("OSM_PATH", DEFAULT_OSM_PATH)),
            bdiff_path: PathBuf::from(env_or_default("BDIFF_PATH", DEFAULT_BDIFF_PATH)),
            promethee_path: PathBuf::from(env_or_default("PROMETHEE_PATH", DEFAULT_PROMETHEE_PATH)),
            corine_path: PathBuf::from(env_or_default("CORINE_PATH", DEFAULT_CORINE_PATH)),
            insee_path: PathBuf::from(env_or_default("INSEE_PATH", DEFAULT_INSEE_PATH)),
            calendar_path: PathBuf::from(env_or_default("CALENDAR_PATH", DEFAULT_CALENDAR_PATH)),
            territory_geojson_path: optional_env("TERRITORY_GEOJSON_PATH").map(PathBuf::from),
            territory_codes: optional_env("TERRITORY_CODES")
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|code| !code.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            territory_label: optional_env("TERRITORY_LABEL"),
            aoi_bbox: parse_env("AOI_BBOX", DEFAULT_AOI_BBOX)?,
            h3_resolution: parse_env("H3_RESOLUTION", DEFAULT_H3_RESOLUTION)?,
            recompute_interval: Duration::from_secs(parse_env(
                "RECOMPUTE_INTERVAL_SECS",
                DEFAULT_RECOMPUTE_INTERVAL_SECS,
            )?),
            risk: RiskConfig {
                fwi_max: parse_env("FWI_MAX", DEFAULT_FWI_MAX)?,
                alpha: parse_env("RISK_ALPHA", DEFAULT_RISK_ALPHA)?,
                beta: parse_env("RISK_BETA", DEFAULT_RISK_BETA)?,
                w_hist: parse_env("RISK_W_HIST", DEFAULT_RISK_W_HIST)?,
                w_wui: parse_env("RISK_W_WUI", DEFAULT_RISK_W_WUI)?,
                w_road: parse_env("RISK_W_ROAD", DEFAULT_RISK_W_ROAD)?,
                w_agri: parse_env("RISK_W_AGRI", DEFAULT_RISK_W_AGRI)?,
            },
            api_bind: parse_env("API_BIND", DEFAULT_API_BIND)?,
            web_assets_dir: PathBuf::from(env_or_default("WEB_ASSETS_DIR", DEFAULT_WEB_ASSETS_DIR)),
            blue_center_enabled: parse_env("BLUE_CENTER_ENABLED", DEFAULT_BLUE_CENTER_ENABLED)?,
            blue_ai_evidence_enabled: parse_env(
                "BLUE_AI_EVIDENCE_ENABLED",
                DEFAULT_BLUE_AI_EVIDENCE_ENABLED,
            )?,
            blue_feux_de_foret_enabled: parse_env(
                "BLUE_FEUX_DE_FORET_ENABLED",
                DEFAULT_BLUE_FEUX_DE_FORET_ENABLED,
            )?,
            openai_api_key: optional_env("OPENAI_API_KEY"),
            blue_openai_model: env_or_default("BLUE_OPENAI_MODEL", DEFAULT_BLUE_OPENAI_MODEL),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn log_summary(&self) {
        tracing::debug!(
            data_profile = %self.data_profile,
            aoi = ?self.aoi_bbox,
            h3_resolution = self.h3_resolution,
            recompute_interval_secs = self.recompute_interval.as_secs(),
            fwi_max = self.risk.fwi_max,
            risk_alpha = self.risk.alpha,
            risk_beta = self.risk.beta,
            risk_w_hist = self.risk.w_hist,
            risk_w_wui = self.risk.w_wui,
            risk_w_road = self.risk.w_road,
            risk_w_agri = self.risk.w_agri,
            firms_configured = self.firms_map_key.is_some(),
            firms_fixture_path = %self.firms_fixture_path.display(),
            meteofrance_configured = self.meteofrance_api_key.is_some(),
            meteofrance_fixture_path = %self.meteofrance_fixture_path.display(),
            backtest_weather_path = %self.backtest_weather_path.display(),
            weather_idw_power = self.weather_idw_power,
            weather_cache_dir = %self.weather_cache_dir.display(),
            osm_path = %self.osm_path.display(),
            bdiff_path = %self.bdiff_path.display(),
            promethee_path = %self.promethee_path.display(),
            corine_path = %self.corine_path.display(),
            insee_path = %self.insee_path.display(),
            calendar_path = %self.calendar_path.display(),
            territory_geojson_path = ?self.territory_geojson_path,
            territory_codes = ?self.territory_codes,
            territory_label = ?self.territory_label,
            blue_ai_evidence_enabled = self.blue_ai_evidence_enabled,
            blue_feux_de_foret_enabled = self.blue_feux_de_foret_enabled,
            openai_configured = self.openai_api_key.is_some(),
            blue_openai_model = %self.blue_openai_model,
            "configuration loaded"
        );
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if !(8..=10).contains(&self.h3_resolution) {
            return Err(ConfigError::Validation(
                "H3_RESOLUTION must be between 8 and 10".to_owned(),
            ));
        }
        if !self.territory_codes.is_empty() && self.territory_geojson_path.is_none() {
            return Err(ConfigError::Validation(
                "TERRITORY_CODES requires TERRITORY_GEOJSON_PATH".to_owned(),
            ));
        }
        if let Some(path) = &self.territory_geojson_path
            && !path.is_file()
        {
            return Err(ConfigError::Validation(format!(
                "TERRITORY_GEOJSON_PATH does not exist: {}",
                path.display()
            )));
        }
        if self.recompute_interval.is_zero() {
            return Err(ConfigError::Validation(
                "RECOMPUTE_INTERVAL_SECS must be greater than zero".to_owned(),
            ));
        }
        if self.blue_openai_model.trim().is_empty() {
            return Err(ConfigError::Validation(
                "BLUE_OPENAI_MODEL must not be blank".to_owned(),
            ));
        }
        if !self.weather_idw_power.is_finite() || self.weather_idw_power <= 0.0 {
            return Err(ConfigError::Validation(
                "WEATHER_IDW_POWER must be finite and greater than zero".to_owned(),
            ));
        }
        if self.risk.fwi_max <= 0.0 {
            return Err(ConfigError::Validation(
                "FWI_MAX must be greater than zero".to_owned(),
            ));
        }
        if self.risk.alpha <= 0.0 || self.risk.beta <= 0.0 {
            return Err(ConfigError::Validation(
                "RISK_ALPHA and RISK_BETA must be greater than zero".to_owned(),
            ));
        }
        for (name, value) in [
            ("RISK_W_HIST", self.risk.w_hist),
            ("RISK_W_WUI", self.risk.w_wui),
            ("RISK_W_ROAD", self.risk.w_road),
            ("RISK_W_AGRI", self.risk.w_agri),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(ConfigError::Validation(format!(
                    "{name} must be between zero and one"
                )));
            }
        }
        Ok(())
    }

    pub fn static_data_paths(&self) -> [StaticDataPath<'_>; 6] {
        [
            StaticDataPath::new("OSM", "OSM_PATH", &self.osm_path),
            StaticDataPath::new("BDIFF", "BDIFF_PATH", &self.bdiff_path),
            StaticDataPath::new("PROMETHEE", "PROMETHEE_PATH", &self.promethee_path),
            StaticDataPath::new("CORINE", "CORINE_PATH", &self.corine_path),
            StaticDataPath::new("INSEE", "INSEE_PATH", &self.insee_path),
            StaticDataPath::new("CALENDAR", "CALENDAR_PATH", &self.calendar_path),
        ]
    }

    pub fn validate_static_data(&self) -> Result<(), ConfigError> {
        if self.data_profile == DataProfile::Fixture {
            return Ok(());
        }
        let invalid = self
            .static_data_paths()
            .into_iter()
            .filter_map(|source| {
                if is_fixture_path(source.path) {
                    Some(format!("{} points to fixture data", source.env_name))
                } else if !source.path.is_file() {
                    Some(format!(
                        "{} does not exist: {}",
                        source.env_name,
                        source.path.display()
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if invalid.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Validation(format!(
                "DATA_PROFILE=production requires real static files ({})",
                invalid.join("; ")
            )))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataProfile {
    Fixture,
    Production,
}

impl std::fmt::Display for DataProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Fixture => "fixture",
            Self::Production => "production",
        })
    }
}

impl FromStr for DataProfile {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fixture" => Ok(Self::Fixture),
            "production" => Ok(Self::Production),
            _ => Err("expected `fixture` or `production`"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StaticDataPath<'a> {
    pub source: &'static str,
    pub env_name: &'static str,
    pub path: &'a std::path::Path,
}

impl<'a> StaticDataPath<'a> {
    const fn new(source: &'static str, env_name: &'static str, path: &'a std::path::Path) -> Self {
        Self {
            source,
            env_name,
            path,
        }
    }
}

pub fn is_fixture_path(path: &std::path::Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "testdata")
}

#[derive(Clone, Copy, Debug)]
pub struct RiskConfig {
    pub fwi_max: f32,
    pub alpha: f32,
    pub beta: f32,
    pub w_hist: f32,
    pub w_wui: f32,
    pub w_road: f32,
    pub w_agri: f32,
}

impl RiskConfig {
    pub const fn heuristic(self) -> risk::HeuristicConfig {
        risk::HeuristicConfig {
            fwi_max: self.fwi_max,
            alpha: self.alpha,
            beta: self.beta,
            w_hist: self.w_hist,
            w_wui: self.w_wui,
            w_road: self.w_road,
            w_agri: self.w_agri,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid value `{value}` for {name}: {reason}")]
    InvalidValue {
        name: &'static str,
        value: String,
        reason: String,
    },
    #[error("invalid configuration: {0}")]
    Validation(String),
}

fn optional_env(name: &'static str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn env_or_default(name: &'static str, default: &str) -> String {
    optional_env(name).unwrap_or_else(|| default.to_owned())
}

fn parse_env<T>(name: &'static str, default: &str) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let value = optional_env(name).unwrap_or_else(|| default.to_owned());
    value
        .parse()
        .map_err(|error: T::Err| ConfigError::InvalidValue {
            name,
            value,
            reason: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{DataProfile, is_fixture_path};
    use grid::BoundingBox;

    #[test]
    fn parses_valid_bbox() {
        let bbox: BoundingBox = "1.68,42.57,3.26,43.46".parse().expect("valid bbox");
        assert!((bbox.west - 1.68).abs() < f64::EPSILON);
        assert!((bbox.north - 43.46).abs() < f64::EPSILON);
    }

    #[test]
    fn rejects_reversed_bbox() {
        let error = "3.26,42.57,1.68,43.46".parse::<BoundingBox>();
        assert!(error.is_err());
    }

    #[test]
    fn parses_data_profiles() {
        assert_eq!("fixture".parse(), Ok(DataProfile::Fixture));
        assert_eq!("PRODUCTION".parse(), Ok(DataProfile::Production));
        assert!("staging".parse::<DataProfile>().is_err());
    }

    #[test]
    fn identifies_fixture_paths_by_component() {
        assert!(is_fixture_path(Path::new("testdata/osm.csv")));
        assert!(is_fixture_path(Path::new("/tmp/project/testdata/osm.csv")));
        assert!(!is_fixture_path(Path::new("data/osm/aude.osm.pbf")));
    }
}
