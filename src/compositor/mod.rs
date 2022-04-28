mod dispatch;

pub mod subsurface;

use std::sync::{atomic::AtomicI32, Mutex};

use wayland_backend::client::InvalidId;
use wayland_client::{
    protocol::{wl_compositor, wl_output, wl_subcompositor, wl_subsurface, wl_surface},
    ConnectionHandle, Dispatch, QueueHandle,
};

use self::subsurface::Subsurface;

/// An error caused by creating a surface.
#[derive(Debug, thiserror::Error)]
pub enum SurfaceError {
    /// The compositor global is not available.
    #[error("the compositor global is not available")]
    MissingCompositorGlobal,

    /// Protocol error.
    #[error(transparent)]
    Protocol(#[from] InvalidId),
}

pub trait CompositorHandler: Sized {
    fn compositor_state(&mut self) -> &mut CompositorState;

    /// The surface has either been moved into or out of an output and the output has a different scale factor.
    fn scale_factor_changed(
        &mut self,
        conn: &mut ConnectionHandle,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    );

    /// A frame callback has been completed.
    ///
    /// This function will be called after sending a [`WlSurface::frame`](wl_surface::WlSurface::frame) request
    /// and committing the surface.
    fn frame(
        &mut self,
        conn: &mut ConnectionHandle,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        time: u32,
    );
}

#[derive(Debug)]
pub struct CompositorState {
    wl_compositor: Option<wl_compositor::WlCompositor>,
    wl_subcompositor: Option<wl_subcompositor::WlSubcompositor>,
    // TODO: Subsurface destroy queue (we need to invoke this on creation of surfaces)
}

impl CompositorState {
    pub fn new() -> CompositorState {
        CompositorState { wl_compositor: None, wl_subcompositor: None }
    }

    pub fn create_surface<D>(
        &self,
        conn: &mut ConnectionHandle,
        qh: &QueueHandle<D>,
    ) -> Result<wl_surface::WlSurface, SurfaceError>
    where
        D: Dispatch<wl_surface::WlSurface, UserData = SurfaceData> + 'static,
    {
        let wl_compositor =
            self.wl_compositor.as_ref().ok_or(SurfaceError::MissingCompositorGlobal)?;

        let surface = wl_compositor.create_surface(
            conn,
            qh,
            SurfaceData { scale_factor: AtomicI32::new(1), outputs: Mutex::new(vec![]) },
        )?;

        Ok(surface)
    }

    /// Adds a subsurface to another surface
    ///
    /// TODO: Double buffered comment
    pub fn add_subsurface<D>(
        &self,
        conn: &mut ConnectionHandle,
        qh: &QueueHandle<D>,
        parent: &wl_surface::WlSurface,
        surface: wl_surface::WlSurface,
    ) -> Result<Subsurface, SurfaceError>
    where
        D: Dispatch<wl_subsurface::WlSubsurface, UserData = ()> + 'static,
    {
        let wl_subcompositor =
            self.wl_subcompositor.as_ref().ok_or(SurfaceError::MissingCompositorGlobal)?;

        let wl_subsurface = wl_subcompositor.get_subsurface(conn, &surface, parent, qh, ())?;

        Ok(Subsurface { wl_surface: surface, wl_subsurface })
    }
}

/// Data associated with a [`WlSurface`](wl_surface::WlSurface).
#[derive(Debug)]
pub struct SurfaceData {
    /// The scale factor of the output with the highest scale factor.
    pub(crate) scale_factor: AtomicI32,

    /// The outputs the surface is currently inside.
    pub(crate) outputs: Mutex<Vec<wl_output::WlOutput>>,
}

#[macro_export]
macro_rules! delegate_compositor {
    ($ty: ty) => {
        type __WlCompositor = $crate::reexports::client::protocol::wl_compositor::WlCompositor;
        type __WlSubcompositor = $crate::reexports::client::protocol::wl_subcompositor::WlSubcompositor;
        type __WlSurface = $crate::reexports::client::protocol::wl_surface::WlSurface;
        type __WlSubsurface = $crate::reexports::client::protocol::wl_subsurface::WlSubsurface;
        type __WlCallback = $crate::reexports::client::protocol::wl_callback::WlCallback;

        $crate::reexports::client::delegate_dispatch!($ty:
            [
                __WlCompositor,
                __WlSubcompositor,
                __WlSurface,
                __WlSubsurface,
                __WlCallback
            ] => $crate::compositor::CompositorState
        );
    };
}
