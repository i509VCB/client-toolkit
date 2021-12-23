use std::{convert::TryInto, marker::PhantomData};

use smithay_client_toolkit::{
    compositor::{CompositorState, SurfaceData, SurfaceDispatch, SurfaceHandler},
    output::{OutputData, OutputDispatch, OutputHandler, OutputInfo, OutputState},
    registry::{RegistryDispatch, RegistryHandle, RegistryHandler},
    shm::{pool::raw::RawPool, ShmDispatch, ShmHandler, ShmState},
    window::{
        DecorationMode, ShellHandler, Window, WindowData, XdgShellDispatch, XdgShellState,
        XdgSurfaceData,
    },
};
use wayland_client::{
    delegate_dispatch,
    protocol::{wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface},
    Connection, ConnectionHandle, Dispatch, QueueHandle,
};
use wayland_protocols::{
    unstable::xdg_decoration::v1::client::{
        zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
    },
    xdg_shell::client::{
        xdg_surface,
        xdg_toplevel::{self, State},
        xdg_wm_base,
    },
};

fn main() {
    env_logger::init();

    let cx = Connection::connect_to_env().unwrap();

    let display = cx.handle().display();

    let mut event_queue = cx.new_event_queue();
    let qh = event_queue.handle();

    let registry = display.get_registry(&mut cx.handle(), &qh, ()).unwrap();

    let mut simple_window = SimpleWindow {
        inner: InnerApp {
            exit: false,
            pool: None,
            width: 256,
            height: 256,
            buffer: None,
            window: None,
        },

        registry_handle: RegistryHandle::new(registry),
        output_state: OutputState::new(),
        compositor_state: CompositorState::new(),
        shm_state: ShmState::new(),
        xdg_shell: XdgShellState::new(),
    };

    event_queue.blocking_dispatch(&mut simple_window).unwrap();
    event_queue.blocking_dispatch(&mut simple_window).unwrap();

    let pool = simple_window
        .shm_state
        .new_raw_pool(
            simple_window.inner.width as usize * simple_window.inner.height as usize * 4,
            &mut cx.handle(),
            &qh,
            (),
        )
        .expect("Failed to create pool");
    simple_window.inner.pool = Some(pool);

    let surface = simple_window.compositor_state.create_surface(&mut cx.handle(), &qh).unwrap();

    let window = simple_window
        .xdg_shell
        .create_window(&mut cx.handle(), &qh, surface.clone(), DecorationMode::ServerDecides)
        .expect("window");

    window.set_title(&mut cx.handle(), "A wayland window");
    // GitHub does not let projects use the `org.github` domain but the `io.github` domain is fine.
    window.set_app_id(&mut cx.handle(), "io.github.smithay.client-toolkit.SimpleWindow");
    window.set_min_size(&mut cx.handle(), (256, 256));

    simple_window.inner.window = Some(window);

    loop {
        event_queue.blocking_dispatch(&mut simple_window).unwrap();

        if simple_window.inner.exit {
            println!("exiting example");
            break;
        }
    }
}

struct SimpleWindow {
    inner: InnerApp,
    registry_handle: RegistryHandle,
    output_state: OutputState,
    compositor_state: CompositorState,
    shm_state: ShmState,
    xdg_shell: XdgShellState,
}

struct InnerApp {
    exit: bool,
    pool: Option<RawPool>,
    width: u32,
    height: u32,
    buffer: Option<wl_buffer::WlBuffer>,
    window: Option<Window>,
}

impl ShmHandler for InnerApp {
    fn supported_format(&mut self, _format: wl_shm::Format) {
        // TODO
    }
}

impl SurfaceHandler for InnerApp {
    fn scale_factor_changed(&mut self, _surface: &wl_surface::WlSurface, _new_factor: i32) {
        // TODO
    }
}

impl OutputHandler for InnerApp {
    fn new_output(&mut self, _info: OutputInfo) {}

    fn update_output(&mut self, _info: OutputInfo) {}

    fn output_destroyed(&mut self, _info: OutputInfo) {}
}

impl ShellHandler<SimpleWindow> for InnerApp {
    fn request_close(&mut self, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        cx: &mut ConnectionHandle,
        qh: &QueueHandle<SimpleWindow>,
        size: (u32, u32),
        _: Vec<State>, // We don't particularly care for the states at the moment.
        window: &Window,
    ) {
        if size == (0, 0) {
            self.width = 256;
            self.height = 256;
        } else {
            self.width = size.0;
            self.height = size.1;
        }

        println!("Configure: ({}x{})", size.0, size.1);

        // Ensure the pool is big enough to hold the new buffer.
        self.pool
            .as_mut()
            .unwrap()
            .resize((self.width * self.height * 4) as usize, cx)
            .expect("resize pool");

        // Destroy the old buffer.
        // FIXME: Integrate this into the pool logic.
        self.buffer.take().map(|buffer| {
            buffer.destroy(cx);
        });

        let (buffer, wl_buffer) = self
            .pool
            .as_mut()
            .unwrap()
            .create_buffer(
                0,
                self.width as i32,
                self.height as i32,
                self.width as i32 * 4,
                wl_shm::Format::Argb8888,
                (),
                cx,
                &qh,
            )
            .expect("create buffer");

        // Draw to the window:
        {
            let width = self.width;
            let height = self.height;

            buffer.chunks_exact_mut(4).enumerate().for_each(|(index, chunk)| {
                let x = (index / width as usize) as u32;
                let y = (index % height as usize) as u32;

                let a = 0xFF;
                let r = u32::min(((width - x) * 0xFF) / width, ((height - y) * 0xFF) / height);
                let g = u32::min((x * 0xFF) / width, ((height - y) * 0xFF) / height);
                let b = u32::min(((width - x) * 0xFF) / width, (y * 0xFF) / height);
                let color = (a << 24) + (r << 16) + (g << 8) + b;

                let array: &mut [u8; 4] = chunk.try_into().unwrap();
                *array = color.to_le_bytes();
            });
        }

        self.buffer = Some(wl_buffer.clone());

        assert!(self.buffer.is_some(), "No buffer?");
        window.wl_surface().attach(cx, self.buffer.clone(), 0, 0);
        window.wl_surface().commit(cx);
    }
}

delegate_dispatch!(SimpleWindow: <UserData = OutputData> [wl_output::WlOutput] => OutputDispatch<'_, InnerApp> ; |app| {
    &mut OutputDispatch(&mut app.output_state, &mut app.inner)
});

delegate_dispatch!(SimpleWindow: <UserData = ()> [wl_compositor::WlCompositor] => SurfaceDispatch<'_, InnerApp> ; |app| {
    &mut SurfaceDispatch(&mut app.compositor_state, &mut app.inner)
});

delegate_dispatch!(SimpleWindow: <UserData = SurfaceData> [wl_surface::WlSurface] => SurfaceDispatch<'_, InnerApp> ; |app| {
    &mut SurfaceDispatch(&mut app.compositor_state, &mut app.inner)
});

delegate_dispatch!(SimpleWindow: <UserData = ()> [wl_shm::WlShm, wl_shm_pool::WlShmPool] => ShmDispatch<'_, InnerApp> ; |app| {
    &mut ShmDispatch(&mut app.shm_state, &mut app.inner)
});

delegate_dispatch!(SimpleWindow: <UserData = ()>
[
    xdg_wm_base::XdgWmBase,
    zxdg_decoration_manager_v1::ZxdgDecorationManagerV1
] => XdgShellDispatch<'_, SimpleWindow, InnerApp> ; |app| {
    &mut XdgShellDispatch(&mut app.xdg_shell, &mut app.inner, PhantomData)
});

delegate_dispatch!(SimpleWindow: <UserData = XdgSurfaceData> [xdg_surface::XdgSurface] => XdgShellDispatch<'_, SimpleWindow, InnerApp> ; |app| {
    &mut XdgShellDispatch(&mut app.xdg_shell, &mut app.inner, PhantomData)
});

delegate_dispatch!(SimpleWindow: <UserData = WindowData> [xdg_toplevel::XdgToplevel, zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1] => XdgShellDispatch<'_, SimpleWindow, InnerApp> ; |app| {
    &mut XdgShellDispatch(&mut app.xdg_shell, &mut app.inner, PhantomData)
});

delegate_dispatch!(SimpleWindow: <UserData = ()> [wl_registry::WlRegistry] => RegistryDispatch<'_, SimpleWindow> ; |app| {
    let handles: Vec<&mut dyn RegistryHandler<SimpleWindow>> = vec![&mut app.xdg_shell, &mut app.shm_state, &mut app.compositor_state];

    &mut RegistryDispatch(&mut app.registry_handle, handles)
});

// TODO
impl Dispatch<wl_buffer::WlBuffer> for SimpleWindow {
    type UserData = ();

    fn event(
        &mut self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &Self::UserData,
        _: &mut wayland_client::ConnectionHandle,
        _: &wayland_client::QueueHandle<Self>,
        _: &mut wayland_client::DataInit<'_>,
    ) {
        // todo
    }
}
