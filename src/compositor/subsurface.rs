use wayland_client::protocol::{wl_subsurface, wl_surface};

#[derive(Debug)]
pub struct Subsurface {
    pub(super) wl_surface: wl_surface::WlSurface,
    pub(super) wl_subsurface: wl_subsurface::WlSubsurface,
}

impl Subsurface {
    pub fn wl_surface(&self) -> &wl_surface::WlSurface {
        &self.wl_surface
    }

    pub fn wl_subsurface(&self) -> &wl_subsurface::WlSubsurface {
        &self.wl_subsurface
    }
}
