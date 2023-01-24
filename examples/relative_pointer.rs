use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_pointer, delegate_registry,
    delegate_relative_pointer, delegate_seat, delegate_shm, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerHandler},
        relative_pointer::{RelativeMotionEvent, RelativePointerHandler, RelativePointerState},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1;

fn main() {
    env_logger::init();

    let conn = Connection::connect_to_env().unwrap();

    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    // The compositor (not to be confused with the server which is commonly called the compositor) allows
    // configuring surfaces to be presented.
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
    // For desktop platforms, the XDG shell is the standard protocol for creating desktop windows.
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg shell is not available");
    // Since we are not using the GPU in this example, we use wl_shm to allow software rendering to a buffer
    // we share with the compositor process.
    let shm = Shm::bind(&globals, &qh).expect("wl shm is not available.");

    let surface = compositor.create_surface(&qh);
    // And then we can create the window.
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    // Configure the window, this may include hints to the compositor about the desired minimum size of the
    // window, app id for WM identification, the window title, etc.
    window.set_title("A wayland window");
    // GitHub does not let projects use the `org.github` domain but the `io.github` domain is fine.
    window.set_app_id("io.github.smithay.client-toolkit.RelativePointer");
    window.set_min_size(Some((256, 256)));
    // In order for the window to be mapped, we need to perform an initial commit with no attached buffer.
    // For more info, see WaylandSurface::commit
    //
    // The compositor will respond with an initial configure that we can then use to present to the window with
    // the correct options.
    window.commit();

    let mut simple_window = SimpleWindow {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        relative_pointer_state: RelativePointerState::bind(&globals, &qh),

        exit: false,
        width: 256,
        height: 256,
        window,
        pointer: None,
        relative_pointer: None,
    };

    while !simple_window.exit {
        event_queue.blocking_dispatch(&mut simple_window).unwrap();
    }
}

struct SimpleWindow {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    relative_pointer_state: RelativePointerState,

    exit: bool,
    width: u32,
    height: u32,
    window: Window,
    pointer: Option<wl_pointer::WlPointer>,
    relative_pointer: Option<zwp_relative_pointer_v1::ZwpRelativePointerV1>,
}

impl CompositorHandler for SimpleWindow {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
        // Not needed for this example.
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }
}

impl OutputHandler for SimpleWindow {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for SimpleWindow {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        match configure.new_size {
            Some(size) => {
                self.width = size.0;
                self.height = size.1;
            }
            None => {
                self.width = 256;
                self.height = 256;
            }
        }

        self.draw(conn, qh);
    }
}

impl SeatHandler for SimpleWindow {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            println!("Set pointer capability");
            let pointer = self.seat_state.get_pointer(qh, &seat).expect("Failed to create pointer");
            let relative_pointer =
                self.relative_pointer_state.get_relative_pointer(&pointer, qh).ok();
            if relative_pointer.is_some() {
                println!("Created relative pointer");
            } else {
                println!("Compositor does not support relative pointer events");
            }
            self.pointer = Some(pointer);
            self.relative_pointer = relative_pointer;
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_some() {
            println!("Unset pointer capability");
            self.pointer.take().unwrap().release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for SimpleWindow {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        _events: &[PointerEvent],
    ) {
    }
}

impl RelativePointerHandler for SimpleWindow {
    fn relative_pointer_motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _relative_pointer: &zwp_relative_pointer_v1::ZwpRelativePointerV1,
        _pointer: &wl_pointer::WlPointer,
        event: RelativeMotionEvent,
    ) {
        println!("{:?}", event);
    }
}

impl ShmHandler for SimpleWindow {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SimpleWindow {
    pub fn draw(&mut self, _conn: &Connection, qh: &QueueHandle<Self>) {
        let window = &self.window;
        let width = self.width;
        let height = self.height;
        let stride = self.width as i32 * 4;

        let mut pool = SlotPool::new(width as usize * height as usize * 4, &self.shm)
            .expect("Failed to create pool");

        let buffer = pool
            .create_buffer(width as i32, height as i32, stride, wl_shm::Format::Xrgb8888)
            .expect("create buffer")
            .0;

        for i in pool.canvas(&buffer).unwrap().chunks_exact_mut(4) {
            i[0] = 255;
            i[1] = 255;
            i[2] = 255;
        }

        // Damage the entire window
        window.wl_surface().damage_buffer(0, 0, self.width as i32, self.height as i32);

        // Request our next frame
        window.wl_surface().frame(qh, window.wl_surface().clone());

        // Attach and commit to present.
        buffer.attach_to(window.wl_surface()).expect("buffer attach");
        window.wl_surface().commit();
    }
}

delegate_compositor!(SimpleWindow);
delegate_output!(SimpleWindow);
delegate_shm!(SimpleWindow);

delegate_seat!(SimpleWindow);
delegate_pointer!(SimpleWindow);
delegate_relative_pointer!(SimpleWindow);

delegate_xdg_shell!(SimpleWindow);
delegate_xdg_window!(SimpleWindow);

delegate_registry!(SimpleWindow);

impl ProvidesRegistryState for SimpleWindow {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState,];
}
