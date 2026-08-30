# FireSift — Roadmap

État au 20 août 2026.

## Principes de progression

- v1 reste actif tant qu'une décision de promotion séparée n'est pas validée.
- Le candidat reste inactif ; aucune phase documentaire ne peut changer ce statut.
- Chaque évolution de modèle doit être réversible, mesurée et indépendante du service v1.
- Les phases d'interface et de documentation ne doivent déclencher ni scoring, ni import, ni migration en production.
- Les tags publiés (`v0.4.2`, `v0.4.2-app`, …, `v0.5.0`) ne doivent pas être déplacés.
- Un flag de déploiement (`*_CONSOLE_ENABLED`, `BLUE_CENTER_ENABLED`, …) n'est **jamais** un mécanisme d'authentification — voir [`docs/architecture.md#api-surfaces`](docs/architecture.md#api-surfaces).

### Légende des statuts utilisés ci-dessous

- **Intégré et publié** — fusionné sur `main` et couvert par une release taguée.
- **Présent dans `Unreleased`** — fusionné/rejoué sur la branche de travail courante, pas encore dans une release taguée ; voir `CHANGELOG.md`.
- **Expérimental** — code réel et fonctionnel, gardé derrière un flag désactivé par défaut, sans garantie de stabilité de contrat.
- **Partiellement implémenté** — une fondation réelle existe, le système complet annoncé par le nom de la phase n'existe pas encore.
- **Non commencé** — aucune implémentation.
- **Hors périmètre** — explicitement exclu de la trajectoire actuelle.

La séquence **stabilisation transverse → 4B → P3** est la recommandation actuelle (voir la nouvelle étape ci-dessous). Elle pourra être réévaluée à partir des observations de chaque étape ; cet ordre n'est pas une obligation irréversible et ne remplace pas les validations propres à chaque phase.

## Terminé et intégré

### Socle opérationnel historique

- Workspace Rust, API Axum et PostgreSQL/PostGIS.
- FWI, grille H3 et ingestion NASA FIRMS.
- Observations Météo-France et prévisions AROME/ARPEGE.
- Features OSM, BDIFF, Prométhée, CORINE, INSEE et calendrier.
- Scoring v1 explicable, API GeoJSON, alertes et dashboard opérationnel.
- Déploiement Docker/Caddy et traitement territorial France métropolitaine.

Les anciennes appellations « phases 0–9C » décrivent ce socle historique. Elles ne constituent plus la roadmap active du programme scientifique.

### Phases 0 à 2 — plateforme de données

- **Phase 0** — sauvegarde et restauration PostgreSQL validées.
- **Phase 1** — schémas `raw`, `staging`, `fire`, validation et opérations.
- **Phase 2** — ingestion FIRMS traçable et opérationnelle.

### Phase 3A — spécification scientifique

- Population, labels, fenêtres temporelles et règles de construction du dataset humain spécifiés.

### Phases 3B.1 à 3B.6 — datasets

- Fondation et audits de qualité BDIFF.
- Versioning des features et des datasets.
- Stratégies de negative sampling N2/N3.
- Variantes strict/inclusive.
- Revue scientifique des biais, dérives et règles de combustibilité.

### Phases 3B.7 à 3B.9 — candidat

- Baselines et candidat GBM avec calibration isotonic.
- Comparaison appariée v1/candidat sur la population historique commune.
- Artefact versionné, checksums, parité entraînement/inférence et plan de promotion.

### Phases 3B.10 et 3B.11 — garde-fous de production

- **P1** — candidat enregistré avec le statut `inactive`.
- **P2** — chargement et validation en lecture seule.
- Aucun scoring candidat et aucune activation.

### Phases 4A à 4A.2 — console scientifique

- API scientifique read-only.
- Console privée, responsive et sans chaîne de build frontend.
- Présentation des sources, imports, qualité, features, datasets, modèles et intégrité.
- Déploiement VPS derrière une protection Caddy.
- Intégration GitHub v0.4.2, CI stricte et tags séparant application et état intégré.

### Phase 4A.3 — Stabilisation de la console scientifique (close pour son périmètre historique)

**Statut : intégré et publié, close pour son périmètre historique.** Le
travail a été audité et déployé le 28 juillet 2026
(`docs/research/phases/PHASE4A3_STABILIZATION_REPORT.md`, commit
`36027bfea23...`, fusionné sur `main`). Périmètre historique : uniquement
la console scientifique privée (`/science`, `/api/science/*`) — la
console territoriale/Client, BLUE et Watch n'existaient pas encore au
moment de cet audit et **n'en font pas partie**.

Objectifs d'origine :

- utiliser la console sur desktop et mobile ;
- vérifier la cohérence entre SQL, API et UI ;
- clarifier les métriques difficiles à interpréter ;
- corriger les états vides, erreurs et régressions d'ergonomie ;
- mesurer les lenteurs des endpoints scientifiques ;
- surveiller les erreurs de scheduler et les rate limits Open-Meteo ;
- consolider le runbook et le monitoring.

Hors périmètre (toujours vrai) :

- nouveau modèle ;
- modification du scoring v1 ;
- activation du candidat ;
- shadow scoring ;
- migration de données non indispensable ;
- visualisation scientifique majeure.

**Critères de sortie — 4 sur 5 explicitement démontrés par le rapport du
28 juillet 2026 :**

- ✅ aucune incohérence connue entre chiffres affichés et sources (15
  faits UI/API/SQL vérifiés, tous `MATCH`) ;
- ✅ erreurs API observables et documentées (rate-limiting Open-Meteo
  classé et documenté) ;
- ✅ parcours principaux utilisables sur mobile et desktop (32
  vérifications Chromium réel, 4 largeurs d'écran) ;
- ⚠️ **limites scientifiques visibles — non explicitement vérifié par ce
  rapport.** Aucune section du document n'audite spécifiquement
  l'affichage des limites scientifiques dans l'UI ; ce point reste à
  contrôler séparément s'il devient bloquant pour une décision future.
- ✅ procédures d'exploitation vérifiées (déploiement contrôlé, rollback
  configuré et testé, sauvegarde vérifiée avant déploiement).

Cette phase est déclarée close pour ce périmètre précis. Elle ne
s'étend pas rétroactivement à la console territoriale/Client, à BLUE ou
à Watch — ces trois surfaces sont apparues après cet audit et doivent
être stabilisées séparément (voir l'étape suivante).

## Stabilisation transverse des surfaces récentes (en cours)

**Statut : partiellement complété.** Couvre les trois surfaces apparues
après l'audit 4A.3 : la console territoriale/Client, BLUE, et Watch. À
finir avant tout élargissement important de Watch, avant la Phase 4B,
avant P3, et avant toute présentation de BLUE comme un système complet
de validation prospective.

Périmètre minimal et état au 24 août 2026 :

- Client, BLUE et Watch utilisés sur desktop et mobile — **en cours** ;
  vérification visuelle assurée directement par le mainteneur, un défaut
  d'overflow mobile sur BLUE déjà détecté et corrigé indépendamment.
- Cohérence des contrats SQL/API/UI (même méthode que l'audit 4A.3) —
  **fait pour BLUE** (bulletins et confirmations ground-truth
  vérifiés en production, correspondance exacte SQL/API) ; **fait pour
  Watch** (recherche de communes et bbox vérifiées en production contre
  `reference.commune_boundaries`, correspondance exacte y compris les cas
  d'erreur `404`/`400`) ; **volontairement différé pour Client** —
  `CLIENT_CONSOLE_ENABLED=false` en production, rien n'y est exposé
  actuellement ; à faire quand cette console sera activée en production,
  plutôt qu'en environnement local isolé.
- États vides et erreurs API correctement gérés et documentés — **fait**
  (état vide BLUE sans bulletin, disclaimer Client manquant, erreurs
  Watch documentées dans `docs/api.md`).
- Visibilité effective des limites scientifiques dans chaque surface —
  **fait** : Client et Watch portaient déjà la mention standard
  « projet de recherche expérimental, pas une alerte officielle » ; BLUE
  ne l'affichait dans aucune de ses quatre vues (Analyse, Tableau,
  Performance, Terrain) et l'affiche désormais explicitement dans les
  quatre.
- Performance des endpoints (`/api/blue/*`, `/api/watch/*`,
  `/api/client/*`) — **fait pour BLUE et Watch** : mesuré en production,
  toutes les routes répondent en moins de ~55 ms (P50 entre 1 et 25 ms
  selon la taille de la réponse, y compris `/api/blue/alerts` à 388 Ko et
  `/api/blue/ground-truth` à 106 Ko) ; **différé pour Client** comme la
  cohérence SQL/API/UI, tant qu'il n'est pas activé en production.
- Fraîcheur des données affichées (`computed_at` vs `valid_at`) — **fait
  pour Watch** (correctif déjà appliqué) et **fait pour BLUE** :
  `forecast_batch_computed_at` et `issued_at` sont identiques sur tous
  les bulletins récents vérifiés (aucune donnée périmée republiée sous
  une date fraîche), et l'UI distingue déjà correctement `issued_at`
  (« émis », date de calcul) de `alert_24h_valid_at`/`alert_48h_valid_at`
  (échéance de la prévision par commune) — pas de confusion des deux
  contrairement au bug initial de Watch ; **pas encore vérifié pour
  Client**, différé pour la même raison.
- Protection par reverse proxy documentée pour chaque surface activée
  publiquement — **fait** : `/science` et `/blue` sont bien derrière
  Basic Auth dans `deploy/oracle/Caddyfile` ; Client et Watch restent
  sans protection dédiée par choix documenté (données publiques).
- Observabilité minimale (logs, erreurs de scheduler pour BLUE) — **fait**
  (correctif de la ligne de statut `weather_forecast` orpheline en
  production, qui faussait le résumé de santé du dashboard opérationnel).

**Mise à jour du 30 août 2026 : la console territoriale/Client a été
supprimée entièrement** (code, routes, tests, flag `CLIENT_CONSOLE_ENABLED`,
assets statiques) — décision du propriétaire du projet dans le cadre
d'une refonte des interfaces web, la console étant désactivée en
production, jamais utilisée, et sans dépendant externe confirmé. Les
points de ce périmètre qui restaient différés pour Client (cohérence
SQL/API/UI, performance, fraîcheur) sont donc **sans objet** plutôt que
« en attente ».

Cette étape est désormais close : tout le périmètre minimal est couvert
pour BLUE et Watch, et Client n'existe plus.

Hors périmètre : nouveau modèle, modification du scoring v1, activation
du candidat, shadow scoring, extension fonctionnelle majeure de BLUE ou
Watch au-delà des corrections nécessaires à la stabilisation elle-même.

## Phase 4B — visualisations scientifiques

**Statut : non commencé.** À ouvrir après la stabilisation transverse
ci-dessus (qui remplace et étend l'ancienne dépendance directe à 4A.3
seule).

Périmètre envisagé :

- cartes H3 et exploration géographique BDIFF ;
- ROC, precision-recall et calibration ;
- distributions des features ;
- comparaison strict/inclusive et N2/N3 ;
- détails des exclusions ;
- historique des imports et erreurs ;
- filtres temporels et territoriaux.

Cette phase reste read-only et ne modifie aucun statut de modèle.

## P3 — shadow scoring limité

**Statut : non commencé.** À ouvrir seulement après la stabilisation
transverse ci-dessus et validation du protocole.

Principes :

- v1 continue de répondre seul ;
- le candidat reçoit les mêmes cas en arrière-plan ;
- aucun score candidat n'est servi aux utilisateurs ;
- les écarts sont persistés dans un stockage dédié ;
- la console présente la comparaison live ;
- toute erreur candidat est isolée du chemin v1.

La phase doit définir avant implémentation :

- population et fréquence de scoring ;
- schéma de stockage et rétention ;
- métriques de dérive et seuils d'alerte ;
- budget de calcul ;
- arrêt d'urgence et rollback ;
- critères de passage ou d'abandon.

## Après P3

1. Observer une fenêtre live suffisante.
2. Comparer performance, calibration, dérive et stabilité opérationnelle.
3. Documenter les écarts et incidents.
4. Décider explicitement entre abandon, nouvel entraînement, prolongation du shadow ou proposition de promotion.
5. Traiter toute activation comme une phase distincte avec validation humaine.

## État des modèles

| Modèle | Registry | Scoring servi | Shadow scoring | Décision |
|---|---|---:|---:|---|
| v1 | actif | oui | n/a | référence opérationnelle |
| `gbm_isotonic_v2` | inactive | non | non | en attente de P3 |

## État des surfaces (au-delà du socle opérationnel)

| Surface | Statut | Flag, défaut | Notes |
|---|---|---|---|
| Console scientifique | Intégré et publié ; stabilisation 4A.3 close pour ce périmètre | `SCIENCE_CONSOLE_ENABLED`, `false` | Voir §4A.3 ci-dessus |
| Console territoriale/Client | **Supprimée** (30 août 2026) | — | Code, routes, tests et flag entièrement retirés ; voir le chantier de consolidation des interfaces web |
| BLUE | Partiellement implémenté (fondation active, enrichie jusqu'à la migration `0032`) | `BLUE_CENTER_ENABLED`, `false` | Voir Phase D ci-dessous et [`docs/architecture.md#blue-forecast-evidence-center`](docs/architecture.md#blue-forecast-evidence-center) ; **insuffisant à lui seul pour déclarer une validation prospective complète** |
| Watch | Présent dans `Unreleased`, expérimental | `WATCH_CONSOLE_ENABLED`, `false` | Implémenté (commit Watch + correctif de fraîcheur), non encore publié dans une release taguée, désactivé par défaut, nécessite la stabilisation transverse ci-dessus avant tout élargissement |

## Open-source track

The scientific/product roadmap above (stabilisation transverse → 4B →
P3) and the open-source readiness track below are independent —
open-sourcing the repository does not accelerate or authorize model
activation, and stabilizing the consoles does not require public
release. Neither track implies commitments beyond what's stated here;
both can be reprioritized independently.

- **Phase A — Open-source readiness** (this work): security audit,
  licensing, documentation reorganization, community files. Tracked in
  [`OPEN_SOURCE_READINESS_REPORT.md`](OPEN_SOURCE_READINESS_REPORT.md).
- **Phase B — Public research release**: make the GitHub repository
  public, cut a tagged release, verify the local demo works from a clean
  checkout. No infrastructure or model change implied.
- **Phase C — Public web platform**: map, forecasts, explainability, a
  public scientific console. Design sketch only today — see
  [`docs/public-platform.md`](docs/public-platform.md).
- **Phase D — Prospective validation**: immutable forecast archive,
  matching against observed events, long-term evaluation. BLUE
  implements a first partial foundation — immutable `+24h`/`+48h`
  evidence archiving, an optional AI-assisted evidence reviewer
  (`BLUE_AI_EVIDENCE_ENABLED`, requires `OPENAI_API_KEY`), and, as of
  migration `0032_blue_community_evidence.sql`, community/terrain-report
  evidence levels (`community_reported`, `press_confirmed`,
  `authority_confirmed`) with a dedicated rejection table for false
  alarms; the full system — reverse matching for recall/specificity and
  a published aggregate track record — is not built yet, and BLUE alone
  does not constitute a complete prospective-validation system — see
  [`docs/scientific-limitations.md`](docs/scientific-limitations.md#prospective-validation-is-partially-implemented-not-complete).
- **Phase E — Shadow candidate** (same content as internal **P3** above):
  the candidate receives live cases, serves no users, and is evaluated for
  drift and calibration.
- **Phase F — Scientific decision**: based on Phase E's results, abandon
  the candidate, retrain it, extend the shadow period, promote it, or
  start a v3. This is always an explicit, documented decision — never
  automatic (see [`GOVERNANCE.md`](GOVERNANCE.md)).

None of these phases are commercial commitments or dated promises — they
describe an intended sequence, not a contract.
