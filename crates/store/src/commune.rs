//! Commune (municipality) boundary reference data.
//!
//! Boundaries are read-only reference data used to clip risk cells and to
//! back commune search/lookup for the Watch public map; nothing in the
//! scoring or scheduler paths writes to this table.

use geo::{BoundingRect as _, Geometry};
use grid::BoundingBox;
use sqlx::Row;

use crate::{Store, StoreError};

/// A commune boundary resolved from `reference.commune_boundaries`.
#[derive(Clone, Debug)]
pub struct CommuneBoundary {
    /// Five-character INSEE municipality code.
    pub insee_code: String,
    /// Commune name.
    pub name: String,
    /// Postal codes served by the commune.
    pub postal_codes: Vec<String>,
    /// Polygonal geometry in WGS84.
    pub geometry: Geometry<f64>,
    /// Bounding box of `geometry`.
    pub bbox: BoundingBox,
}

/// One name-search match against the commune catalog. Deliberately
/// lighter than [`CommuneBoundary`]/[`CommuneCatalogEntry`] -- no
/// geometry is parsed or returned, so this stays cheap for autocomplete.
#[derive(Clone, Debug)]
pub struct CommuneSearchResult {
    pub insee_code: String,
    pub name: String,
    pub department_code: Option<String>,
}

/// One versioned commune and its deterministic H3-centroid coverage.
#[derive(Clone, Debug)]
pub struct CommuneCatalogEntry {
    pub insee_code: String,
    pub name: String,
    pub department_code: Option<String>,
    pub region_code: Option<String>,
    pub boundary: serde_json::Value,
    pub h3_cells: Vec<i64>,
}

impl Store {
    /// Atomically replaces the current commune-to-H3 mapping and upserts the
    /// corresponding versioned boundary catalog.
    ///
    /// # Errors
    ///
    /// Returns an error on an empty catalog, duplicate H3 ownership, invalid
    /// metadata, or database failure.
    pub async fn replace_commune_catalog(
        &self,
        entries: &[CommuneCatalogEntry],
        h3_resolution: i16,
        source_version: &str,
        source_checksum: &str,
    ) -> Result<(u64, u64), StoreError> {
        if entries.is_empty()
            || source_version.trim().is_empty()
            || source_checksum.trim().is_empty()
        {
            return Err(StoreError::InvalidCommuneBoundary(
                "commune catalog metadata is incomplete".to_owned(),
            ));
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('erytheon_commune_catalog'))")
            .execute(&mut *transaction)
            .await?;
        let mut boundary_count = 0_u64;
        for chunk in entries.chunks(500) {
            let codes = chunk
                .iter()
                .map(|entry| entry.insee_code.as_str())
                .collect::<Vec<_>>();
            let names = chunk
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>();
            let departments = chunk
                .iter()
                .map(|entry| entry.department_code.as_deref())
                .collect::<Vec<_>>();
            let regions = chunk
                .iter()
                .map(|entry| entry.region_code.as_deref())
                .collect::<Vec<_>>();
            let boundaries = chunk
                .iter()
                .map(|entry| &entry.boundary)
                .collect::<Vec<_>>();
            boundary_count += sqlx::query(
                "INSERT INTO reference.commune_boundaries(
                    insee_code,name,boundary,department_code,region_code,
                    source_version,source_checksum,updated_at)
                 SELECT code,name,boundary,department,region,$6,$7,NOW()
                 FROM UNNEST($1::text[],$2::text[],$3::text[],$4::text[],$5::jsonb[])
                    AS input(code,name,department,region,boundary)
                 ON CONFLICT(insee_code) DO UPDATE SET name=EXCLUDED.name,
                    boundary=EXCLUDED.boundary,department_code=EXCLUDED.department_code,
                    region_code=EXCLUDED.region_code,source_version=EXCLUDED.source_version,
                    source_checksum=EXCLUDED.source_checksum,updated_at=NOW()",
            )
            .bind(codes)
            .bind(names)
            .bind(departments)
            .bind(regions)
            .bind(boundaries)
            .bind(source_version)
            .bind(source_checksum)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        }
        sqlx::query("DELETE FROM reference.commune_h3_cells WHERE h3_resolution=$1")
            .bind(h3_resolution)
            .execute(&mut *transaction)
            .await?;
        let mut mapping_count = 0_u64;
        let mappings = entries
            .iter()
            .flat_map(|entry| {
                entry
                    .h3_cells
                    .iter()
                    .map(move |h3| (entry.insee_code.as_str(), *h3))
            })
            .collect::<Vec<_>>();
        for chunk in mappings.chunks(10_000) {
            let codes = chunk.iter().map(|(code, _)| *code).collect::<Vec<_>>();
            let cells = chunk.iter().map(|(_, h3)| *h3).collect::<Vec<_>>();
            mapping_count += sqlx::query(
                "INSERT INTO reference.commune_h3_cells(insee_code,h3,h3_resolution)
                 SELECT code,h3,$3 FROM UNNEST($1::text[],$2::bigint[]) input(code,h3)",
            )
            .bind(codes)
            .bind(cells)
            .bind(h3_resolution)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        }
        transaction.commit().await?;
        Ok((boundary_count, mapping_count))
    }
    /// Inserts or replaces a commune boundary.
    ///
    /// `boundary` must be a `GeoJSON` `Polygon` or `MultiPolygon` geometry
    /// object (not a `Feature` or `FeatureCollection`).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database operation fails.
    pub async fn upsert_commune_boundary(
        &self,
        insee_code: &str,
        name: &str,
        postal_codes: &[String],
        boundary: &serde_json::Value,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO reference.commune_boundaries
                (insee_code, name, postal_codes, boundary, updated_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (insee_code) DO UPDATE SET
                name = EXCLUDED.name,
                postal_codes = EXCLUDED.postal_codes,
                boundary = EXCLUDED.boundary,
                updated_at = NOW()",
        )
        .bind(insee_code)
        .bind(name)
        .bind(postal_codes)
        .bind(boundary)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Finds communes whose name starts with `prefix` (case-insensitive),
    /// ordered alphabetically. Used to back name-search autocomplete;
    /// returns no geometry, so it stays cheap regardless of catalog size.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database operation fails.
    pub async fn search_communes(
        &self,
        prefix: &str,
        limit: i64,
    ) -> Result<Vec<CommuneSearchResult>, StoreError> {
        let rows = sqlx::query(
            "SELECT insee_code, name, department_code
               FROM reference.commune_boundaries
              WHERE name ILIKE $1 || '%'
              ORDER BY name
              LIMIT $2",
        )
        .bind(prefix)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(CommuneSearchResult {
                    insee_code: row.try_get("insee_code")?,
                    name: row.try_get("name")?,
                    department_code: row.try_get("department_code")?,
                })
            })
            .collect()
    }

    /// Looks up a commune boundary by INSEE code.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database operation fails or the
    /// persisted boundary is not a valid, non-empty polygonal geometry.
    pub async fn commune_boundary(
        &self,
        insee_code: &str,
    ) -> Result<Option<CommuneBoundary>, StoreError> {
        let Some(row) = sqlx::query(
            "SELECT name, postal_codes, boundary
               FROM reference.commune_boundaries
              WHERE insee_code = $1",
        )
        .bind(insee_code)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let name: String = row.try_get("name")?;
        let postal_codes: Vec<String> = row.try_get("postal_codes")?;
        let boundary: serde_json::Value = row.try_get("boundary")?;
        let geometry: Geometry<f64> = serde_json::from_value::<geojson::Geometry>(boundary)
            .map_err(|error| StoreError::InvalidCommuneBoundary(error.to_string()))?
            .try_into()
            .map_err(|error: geojson::Error| {
                StoreError::InvalidCommuneBoundary(error.to_string())
            })?;
        if !grid::is_polygonal(&geometry) {
            return Err(StoreError::InvalidCommuneBoundary(format!(
                "commune {insee_code} boundary is not polygonal"
            )));
        }
        let rectangle = geometry.bounding_rect().ok_or_else(|| {
            StoreError::InvalidCommuneBoundary(format!("commune {insee_code} boundary is empty"))
        })?;
        let bbox = BoundingBox::new(
            rectangle.min().x,
            rectangle.min().y,
            rectangle.max().x,
            rectangle.max().y,
        )?;
        Ok(Some(CommuneBoundary {
            insee_code: insee_code.to_owned(),
            name,
            postal_codes,
            geometry,
            bbox,
        }))
    }
}
