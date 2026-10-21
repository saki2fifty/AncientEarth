//! Bundled surface data. All geometry and datum calculations live in earth-math.
use earth_math::{SurfaceKind, TerrainError, TerrainPatch, TerrainScene, WaterKind};
use std::sync::Arc;

#[cfg_attr(feature = "desktop", derive(bevy::prelude::Resource))]
pub struct WorldSurface {
    boundaries: Arc<TerrainScene>,
    reference: Arc<TerrainScene>,
}

impl WorldSurface {
    pub fn bundled() -> Result<Self, TerrainError> {
        Ok(Self {
            boundaries: Arc::new(TerrainScene::boundary_only(vec![
                TerrainPatch::from_boundary_bytes(include_bytes!(
                    "../assets/boundaries/region-01.aeb"
                ))?,
                TerrainPatch::from_boundary_bytes(include_bytes!(
                    "../assets/boundaries/region-02.aeb"
                ))?,
                TerrainPatch::from_boundary_bytes(include_bytes!(
                    "../assets/boundaries/region-03.aeb"
                ))?,
                TerrainPatch::from_boundary_bytes(include_bytes!(
                    "../assets/boundaries/region-04.aeb"
                ))?,
            ])?),
            reference: Arc::new(TerrainScene::boundary_only(vec![])?),
        })
    }

    pub fn active(&self, boundaries_enabled: bool) -> &TerrainScene {
        if boundaries_enabled {
            &self.boundaries
        } else {
            &self.reference
        }
    }

    /// Share immutable canonical data with background presentation workers.
    pub fn shared(&self, boundaries_enabled: bool) -> Arc<TerrainScene> {
        Arc::clone(if boundaries_enabled {
            &self.boundaries
        } else {
            &self.reference
        })
    }

    pub fn label(kind: SurfaceKind) -> &'static str {
        match kind {
            SurfaceKind::Terrain(_) => "Mapped land / WGS84 h=0",
            SurfaceKind::Water {
                kind: WaterKind::Ocean,
                ..
            } => "Mapped ocean / WGS84 h=0",
            SurfaceKind::Water {
                kind: WaterKind::Lake,
                ..
            } => "Mapped lake / WGS84 h=0",
            SurfaceKind::Water {
                kind: WaterKind::River,
                ..
            } => "Mapped river / WGS84 h=0",
            SurfaceKind::ReferenceEllipsoid => "WGS84 h=0 / category not mapped",
        }
    }
}
