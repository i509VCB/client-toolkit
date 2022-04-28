use std::sync::atomic::Ordering;

use wayland_client::{
    protocol::{
        wl_callback, wl_compositor, wl_output, wl_subcompositor, wl_subsurface, wl_surface,
    },
    ConnectionHandle, DelegateDispatch, DelegateDispatchBase, Dispatch, Proxy, QueueHandle,
};

use crate::{
    output::OutputData,
    registry::{ProvidesRegistryState, RegistryHandler},
};

use super::{CompositorHandler, CompositorState, SurfaceData};

impl DelegateDispatchBase<wl_surface::WlSurface> for CompositorState {
    type UserData = SurfaceData;
}

impl<D> DelegateDispatch<wl_surface::WlSurface, D> for CompositorState
where
    D: Dispatch<wl_surface::WlSurface, UserData = Self::UserData>
        + Dispatch<wl_output::WlOutput, UserData = OutputData>
        + CompositorHandler,
{
    fn event(
        state: &mut D,
        surface: &wl_surface::WlSurface,
        event: wl_surface::Event,
        data: &Self::UserData,
        conn: &mut ConnectionHandle,
        qh: &QueueHandle<D>,
    ) {
        let mut outputs = data.outputs.lock().unwrap();

        match event {
            wl_surface::Event::Enter { output } => {
                outputs.push(output);
            }

            wl_surface::Event::Leave { output } => {
                outputs.retain(|o| o != &output);
            }

            _ => unreachable!(),
        }

        // Compute the new max of the scale factors for all outputs this surface is displayed on.
        let current = data.scale_factor.load(Ordering::SeqCst);

        let largest_factor = outputs
            .iter()
            .filter_map(|output| output.data::<OutputData>().map(OutputData::scale_factor))
            .reduce(i32::max);

        // Drop the mutex before we send of any events.
        drop(outputs);

        // If no scale factor is found, because the surface has left it's only output, do not change the scale factor.
        if let Some(factor) = largest_factor {
            data.scale_factor.store(factor, Ordering::SeqCst);

            if current != factor {
                state.scale_factor_changed(conn, qh, surface, factor);
            }
        }
    }
}

impl DelegateDispatchBase<wl_subsurface::WlSubsurface> for CompositorState {
    type UserData = ();
}

impl<D> DelegateDispatch<wl_subsurface::WlSubsurface, D> for CompositorState
where
    D: Dispatch<wl_subsurface::WlSubsurface, UserData = ()>,
{
    fn event(
        _: &mut D,
        _: &wl_subsurface::WlSubsurface,
        _: wl_subsurface::Event,
        _: &Self::UserData,
        _: &mut ConnectionHandle,
        _: &QueueHandle<D>,
    ) {
        unreachable!("wl_subsurface has no events")
    }
}

impl DelegateDispatchBase<wl_compositor::WlCompositor> for CompositorState {
    type UserData = ();
}

impl<D> DelegateDispatch<wl_compositor::WlCompositor, D> for CompositorState
where
    D: Dispatch<wl_compositor::WlCompositor, UserData = Self::UserData> + CompositorHandler,
{
    fn event(
        _: &mut D,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &mut ConnectionHandle,
        _: &QueueHandle<D>,
    ) {
        unreachable!("wl_compositor has no events")
    }
}

impl DelegateDispatchBase<wl_subcompositor::WlSubcompositor> for CompositorState {
    type UserData = ();
}

impl<D> DelegateDispatch<wl_subcompositor::WlSubcompositor, D> for CompositorState
where
    D: Dispatch<wl_subcompositor::WlSubcompositor, UserData = Self::UserData> + CompositorHandler,
{
    fn event(
        _: &mut D,
        _: &wl_subcompositor::WlSubcompositor,
        _: wl_subcompositor::Event,
        _: &(),
        _: &mut ConnectionHandle,
        _: &QueueHandle<D>,
    ) {
        unreachable!("wl_subcompositor has no events")
    }
}

impl DelegateDispatchBase<wl_callback::WlCallback> for CompositorState {
    type UserData = wl_surface::WlSurface;
}

impl<D> DelegateDispatch<wl_callback::WlCallback, D> for CompositorState
where
    D: Dispatch<wl_callback::WlCallback, UserData = Self::UserData> + CompositorHandler,
{
    fn event(
        state: &mut D,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        surface: &Self::UserData,
        conn: &mut ConnectionHandle,
        qh: &QueueHandle<D>,
    ) {
        match event {
            wl_callback::Event::Done { callback_data } => {
                state.frame(conn, qh, surface, callback_data);
            }

            _ => unreachable!(),
        }
    }
}

impl<D> RegistryHandler<D> for CompositorState
where
    D: Dispatch<wl_compositor::WlCompositor, UserData = ()>
        + Dispatch<wl_subcompositor::WlSubcompositor, UserData = ()>
        + CompositorHandler
        + ProvidesRegistryState
        + 'static,
{
    fn new_global(
        state: &mut D,
        conn: &mut ConnectionHandle,
        qh: &QueueHandle<D>,
        name: u32,
        interface: &str,
        version: u32,
    ) {
        match interface {
            "wl_compositor" => {
                if state.compositor_state().wl_compositor.is_some() {
                    return;
                }

                let compositor = state
                    .registry()
                    .bind_once::<wl_compositor::WlCompositor, _, _>(
                        conn,
                        qh,
                        name,
                        u32::min(version, 4),
                        (),
                    )
                    .expect("Failed to bind global");

                state.compositor_state().wl_compositor = Some(compositor);
            }

            "wl_subcompositor" => {
                if state.compositor_state().wl_subcompositor.is_some() {
                    return;
                }

                let subcompositor = state
                    .registry()
                    .bind_once::<wl_subcompositor::WlSubcompositor, _, _>(
                        conn,
                        qh,
                        name,
                        u32::min(version, 1),
                        (),
                    )
                    .expect("Failed to bind global");

                state.compositor_state().wl_subcompositor = Some(subcompositor);
            }

            _ => (),
        }
    }

    fn remove_global(
        _state: &mut D,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<D>,
        _name: u32,
    ) {
        // wl_compositor and wl_subcompositor are capability globals
    }
}
