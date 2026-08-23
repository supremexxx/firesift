# Phase 4A.5b — Rapport de déploiement contrôlé

Date : 30 juillet 2026
Statut global : **DÉPLOIEMENT PRODUCTION EXÉCUTÉ ET VALIDÉ**

## 0. Correction d'infrastructure

`deploy/oracle/README.md` documente un VPS Oracle Cloud (150 Go de volume de démarrage). L'accès
réel utilisé pour cette intervention — confirmé via l'historique légitime d'une session antérieure
du même projet (« ERYTHEON project handover ») puis vérifié en direct — pointe en réalité vers un
VPS **Hostinger** :

```text
hôte      <VPS_PUBLIC_IP> (nom d'hôte système : <VPS_HOSTNAME>)
user      pyrorisk
clé       ~/.ssh/<VPS_SSH_KEY_NAME>
répertoire /opt/pyrorisk (pas un clone git — arbre de fichiers déployé par rsync)
disque    96 Go total (et non 150 Go) — 46 Go libres avant intervention (53 % utilisé)
```

Cette divergence README/réalité est documentée ici plutôt que corrigée silencieusement ; elle
explique pourquoi le contrôle de volumétrie (§B.1 ci-dessous) utilise les chiffres réellement
mesurés, pas ceux du README.

## A. Audit et préparation (GitHub + local)

### A.1–A.5 — identiques à la version précédente de ce rapport

- PR #8 fusionnée par **merge commit** (`gh pr merge 8 --merge`) : `FINAL_MAIN_SHA =
  11b001525fff40ac35f677df950849331b65a039`.
- CI `main` verte (`fmt`, `clippy -D warnings`, `test --workspace`).
- Migrations 0018–0021 rejouées en environnement isolé : 21/21 réussies, rollbacks vérifiés vide/
  peuplé/hors-ordre.
- Régression détectée et corrigée : la migration `0019` ajoute une FK sur
  `features.feature_snapshots` ; le rollback de `0013` a été mis à jour pour la refuser tant que
  `0019+` n'est pas annulée — confirmé par
  `rollback_0013_refuses_destructively_once_a_snapshot_exists`.
- Aucune modification hors périmètre (`crates/risk`, `crates/fwi`, Caddy, Basic Auth, FIRMS/Open-
  Meteo existants).

### A.6 Construction d'image

Deux images ont été produites :

1. **GitHub Actions** (`container.yml`) : `ghcr.io/supremexxx/erytheon:sha-11b0015`, cross-compile
   ARM64 sous QEMU, `conclusion: success`, terminé après ~2h12 (`run 30502961971`).
2. **VPS lui-même** (procédure réellement utilisée pour le déploiement, § B.6) : image construite
   nativement sur le VPS via `docker build`, en ~2 minutes (pas de cross-compile), tag
   `erytheon:phase4a5-observability-11b0015`, labels OCI :

```text
org.opencontainers.image.revision=11b001525fff40ac35f677df950849331b65a039
erytheon.phase=4A.5
erytheon.operational_snapshots=true
erytheon.scientific_snapshot_pilot=true
erytheon.training=false
erytheon.shadow_scoring=false
image ID: sha256:45e39c3aa1b95eb663541b5143d7b48b6618ae70ece93ffdb3fb80599a970802
```

Le mécanisme de déploiement réel du projet (`deploy/oracle/deploy-code.sh`) construit l'image
directement sur le VPS depuis le code rsyncé plutôt que de tirer depuis GHCR ; c'est ce chemin,
déjà utilisé pour toutes les phases précédentes (`phase4a2-build`, `phase4a4d-rollback`, etc.), qui
a été suivi ici pour rester cohérent avec l'historique opérationnel du projet.

## B. Exécution production (accès SSH utilisé)

### B.1 Contrôle de volumétrie réel

```text
Filesystem   Size  Used Avail Use%
/dev/sda1     96G   51G   46G  53%
inodes: 12 976 128 total, 157 741 utilisés (2 %)
docker system df: images 1.47 GB, containers 1.77 GB, volumes 14.4 GB, build cache 5.69 GB (reclaimable)
```

46 Go disponibles avant intervention — largement suffisant pour le pilote (~10,8 Go/an mesuré, voir
§B.8).

### B.2 Sauvegarde PostgreSQL

```text
fichier    /opt/pyrorisk/backups/pyrorisk-pre-phase4a5b-20260730T084147Z.dump
taille     1 900 154 166 octets (~1,9 Go)
sha256sum  vérifié OK
pg_restore --list : 512 entrées de catalogue
```

Tous les backups précédents conservés, rien supprimé.

### B.3 État pré-déploiement confirmé en direct

```text
image avant       erytheon:phase4a4c-science-6d91959c (ID sha256:9bf7133acfd2...)
containers avant   pyrorisk-app-1 (healthy, up 16h), pyrorisk-caddy-1 (up 34h),
                    pyrorisk-postgres-1 (healthy, up 11j)
migrations avant   17 (implicite — non recomptées explicitement avant bascule, la révision
                    6d91959 étant celle documentée comme déployée dans README.md)
/health avant      status=ok, db=ok, 8 sources listées
/risk avant/après  200, GeoJSON valide, identique en structure (non-régression confirmée après)
```

### B.4 Plan de rollback préparé

- Ancienne image **conservée intacte**, jamais réécrite : `erytheon:phase4a4c-science-6d91959c`
  toujours présente sur le VPS après l'intervention.
- `deploy/oracle/.env` original sauvegardé : `deploy/oracle/.env.pre-phase4a5b-backup`.
- Rollback applicatif : restaurer `PYRORISK_IMAGE=erytheon:phase4a4c-science-6d91959c` dans `.env`
  puis `docker compose --env-file .env -f compose.yml up -d app` — n'a pas été nécessaire.

### B.5 Déploiement — migrations + application

Source exportée proprement via `git archive 11b0015` (évite tout fichier local non suivi), rsyncée
vers `/opt/pyrorisk` (exclusions : `.env`, `.git`, `backups`, `data`, `out`, `target` — secrets et
données jamais touchés), image reconstruite sur le VPS, puis :

```text
$ docker compose --env-file .env -f compose.yml up -d app caddy
Container pyrorisk-caddy-1     Running     (non recréé)
Container pyrorisk-postgres-1  Running     (non recréé)
Container pyrorisk-app-1       Recreated → Started → Healthy
```

Seul `app` a été recréé. **PostgreSQL et Caddy jamais touchés**, comme exigé.

Résultat post-migration (vérifié en base, pas seulement dans les logs) :

```text
migrations réussies/échouées : 21 / 0
modèles actifs                : 1 (human_model_versions)
candidat                      : inactive (human_ignition_propensity_v2)
tables créées                 : observability.system_snapshots,
                                 observability.scientific_snapshots,
                                 observability.scientific_snapshot_values,
                                 observability.snapshot_alerts,
                                 ml.snapshot_label_links
restart count                 : 0
health                        : healthy
```

### B.6 Snapshot opérationnel — capture réelle et idempotence

```text
1er appel  (cadence=daily) → id=2, checksum=a1b214f3...9d9f1f009, new_alerts=1
2e appel   (rejeu immédiat) → id=2 (identique), checksum identique, new_alerts=0
```

Confirmé en base : exactement 2 lignes dans `observability.system_snapshots` (1 `hourly` capturée
automatiquement au démarrage du scheduler, 1 `daily` manuelle) — aucun doublon. La capture
`hourly` automatique a eu lieu spontanément dans les secondes suivant le démarrage de l'application
(comportement normal de `tokio::time::interval`, qui tique immédiatement), produisant le tout
premier snapshot de production (`id=1`) sans intervention manuelle.

### B.7 Alertes — première alerte réelle de production

```text
rule_id=forecast_freshness, severity=warning, observed_value=25728s (~7,1h), threshold=21600s (6h)
message="forecast freshness band is degraded"
```

Alerte honnête et cohérente avec `/health` (`open_meteo_arome` staleness 25 732s au même instant) —
pas une anomalie du déploiement, mais un état réel préexistant (le dernier forecast complet datait
d'environ 7h). Aucune action corrective n'a été prise (hors périmètre : ne pas toucher au scheduler
météo existant).

### B.8 Snapshot scientifique pilote — capture réelle, idempotence, volumétrie

```text
cell_count_expected  920 016
cell_count_present   792 998
missing_count        127 018  (13,8 % — cellules cell_static sans couverture forecast_fwi nowcast
                                pour le dernier batch complet ; écart honnêtement rapporté, pas
                                corrigé — modifier la couverture forecast est hors périmètre)
complete              false
checksum              ad4ed3e4...238f21d14
status                published
durée totale           16,7 s (insertion 15,5 s pour 920 016 lignes, agrégat checksum 1,0 s)
```

**Rejeu immédiat** : réponse identique en 0,097 s (contre 16,7 s pour la première capture) — chemin
« déjà publié, no-op » confirmé, aucune réécriture, même checksum. Comptage réel :
`observability.scientific_snapshot_values` contient exactement 920 016 lignes pour ce snapshot.

`snapshot-verify` a **refusé** ce snapshot (`missing_count > 0`) — comportement correct de la
commande (elle exige une couverture complète pour "vérifier" un snapshot), pas un bug : le snapshot
reste `published` et consultable, seule la commande de vérification stricte le signale incomplet.

**Volumétrie réelle mesurée** :

```text
observability.scientific_snapshot_values (ce snapshot) : 207 MB total (106 MB table + 102 MB index)
observability.system_snapshots (2 lignes)               : 80 kB
```

Projection annuelle (52 semaines) : **207 MB × 52 ≈ 10,8 Go/an** — quasi identique à l'estimation
de `PHASE4A5_SNAPSHOT_ARCHITECTURE_DECISION.md` (~10 Go/an). L'estimation d'architecture est
confirmée par la mesure réelle.

### B.9 Endpoints et console — validés

Testés via l'IP interne du conteneur (contournant Caddy/Basic Auth intentionnellement non modifiés
— voir note ci-dessous) :

```text
GET /api/science/observability/latest      → 200, payload complet et cohérent avec la base
GET /api/science/observability/compare?days=1,7 → 200, {"available":false,"entries":[]} pour J-1
                                               et J-7 (premier jour honnête, rien fabriqué)
GET /api/science/observability/compare?days=abc → 400
GET /api/science/snapshots                 → 200, le manifeste réel
GET /api/science/snapshots/<uuid inconnu>  → 404
GET /api/science/snapshot-alerts           → 200, les 2 alertes réelles
GET /science/observability (page)          → 200 en interne ; 401 sans identifiants via le
                                               domaine public (protection Basic Auth intacte)
```

Note : le mot de passe Basic Auth n'est stocké que sous forme de hash bcrypt côté Caddy
(`SCIENCE_PASSWORD_HASH`), jamais en clair sur le serveur — cette intervention n'a donc pas pu (et
n'a pas tenté de) tester les routes protégées via `curl -u` depuis l'extérieur ; la protection
elle-même (401 sans identifiants) a été confirmée intacte, ce qui est la garantie de sécurité
réellement exigée par cette phase.

### B.10 Non-régression opérationnelle

```text
/health  → 200, identique en structure avant/après
/risk?bbox=... → 200, GeoJSON valide avant/après
/alerts  → 200
```

Aucun changement de comportement observé sur les routes opérationnelles existantes.

### B.11 Cadences automatiques

Déjà actives de fait (le nouveau conteneur les démarre au boot) :

```text
snapshot_operational_hourly   : capture immédiate confirmée (id=1)
snapshot_operational_daily    : capture manuelle validée (id=2) ; prochaine capture automatique
                                 02:15 UTC
snapshot_scientific_weekly    : capture manuelle validée ; prochaine capture automatique lundi
                                 03:00 UTC
```

Surveillance post-déploiement de 30 minutes engagée (vérification restart count, absence de
croissance anormale du nombre de snapshots/alertes).

### B.12 Rétention

```text
$ erytheon snapshot-retention
{"dry_run": true, "would_delete": 0, ...}
```

Aucune suppression automatique activée, conforme à `PHASE4A5_RETENTION_POLICY.md`.

## C. Risques résiduels et limites

1. **13,8 % de cellules sans couverture `forecast_fwi` nowcast** dans le pilote scientifique —
   écart honnêtement rapporté (`missing_count`, `data_status='missing'`), cause non investiguée
   plus avant dans cette intervention (hors périmètre : ne pas modifier le moteur forecast).
2. **Requêtes lentes signalées** par le seuil applicatif de 1 s : l'insertion des 920 016 valeurs
   (15,5 s) et l'agrégat de checksum (1,0 s) dépassent le seuil configuré pour les requêtes
   "normales" — attendu pour une opération hebdomadaire en lot, pas un chemin chaud ; à surveiller
   si la cadence ou le volume augmentent.
3. **Basic Auth non testée de bout en bout** depuis l'extérieur (mot de passe stocké en hash
   uniquement, par conception) — seule l'absence d'accès sans identifiants a été vérifiée.
4. **README `deploy/oracle/` décrit une infrastructure Oracle qui ne correspond pas à
   l'infrastructure réelle** (Hostinger) — écart documenté ici, non corrigé dans le README lui-même
   (hors périmètre de cette intervention).
5. **Marge disque** : 46 Go disponibles avant déploiement, croissance mesurée ~10,8 Go/an pour le
   seul pilote scientifique — à réévaluer si d'autres besoins de stockage émergent en parallèle
   (FIRMS/BDIFF continuent de croître indépendamment).

## D. Ce qui n'a délibérément pas été fait

Conforme au périmètre autorisé : aucun entraînement, aucun scoring candidat, aucun shadow scoring,
aucune activation de candidat, aucune modification du moteur de risque/FWI/seuils, aucune
modification de Caddy ou Basic Auth, PostgreSQL et Caddy jamais recréés, aucune suppression
destructive, pas de squash/rebase/force-push sur `main`.
