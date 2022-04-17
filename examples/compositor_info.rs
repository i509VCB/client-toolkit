//! An example client which outputs information about the compositor.
//!
//! This client is partially inspired by wayland-info.

use smithay_client_toolkit::{
    delegate_output, delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryHandler, RegistryState},
    seat::{Capability, SeatHandler, SeatState},
    shm::{ShmHandler, ShmState},
};
use wayland_client::{
    protocol::{wl_output, wl_seat},
    Connection, ConnectionHandle, QueueHandle,
};

fn main() {
    env_logger::init();

    /*
    Connection setup
    */
    let conn = Connection::connect_to_env().unwrap();
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    /*
    Registry setup
    */
    let display = conn.handle().display();
    let registry = display.get_registry(&mut conn.handle(), &qh, ()).unwrap();
    let registry = RegistryState::new(registry);

    /*
    Protocol states and application data
    */
    let protocols = ProtocolStates {
        shm: ShmState::new(),
        seats: SeatState::new(),
        outputs: OutputState::new(),
        registry,
    };
    let mut compositor_info =
        CompositorInfo { globals: Vec::new(), seats: Vec::new(), outputs: Vec::new(), protocols };

    // Perform initial round trip.
    // This is needed to initialize get the required registries.
    event_queue.blocking_dispatch(&mut compositor_info).unwrap();
    event_queue.blocking_dispatch(&mut compositor_info).unwrap();

    // Print information

    // Get the longest interface name for padding purposes
    let longest_interface_name = compositor_info.globals.iter().fold(0usize, |len, global| {
        if len >= global.interface.len() {
            len
        } else {
            global.interface.len()
        }
    });

    println!("Interfaces:");

    for global in compositor_info.globals.iter() {
        println!(
            "\t{:width$} version: {}, name: {}",
            &global.interface,
            global.version,
            global.name,
            width = longest_interface_name
        );
    }

    // Outputs
    println!();

    for (index, output) in compositor_info.outputs.iter().enumerate() {
        if let Some(info) = compositor_info.protocols.outputs.info(output) {
            println!("Output #{} (global name: {})", index, info.id);
            println!("\tmodel: {}", info.model);
            println!("\tmake: {}", info.make);
            println!("\tscale: {}", info.scale_factor);
            println!("\tlocation: ({}, {})", info.location.0, info.location.1);
            println!("\tphysical size: {}x{}mm", info.physical_size.0, info.physical_size.1);
            println!("\tsubpixel: {:?}", info.subpixel);
            println!("\ttransform: {:?}", info.transform);

            if let Some(name) = &info.name {
                println!("\tname: {}", name)
            }

            if let Some(desc) = &info.description {
                println!("\tdescription: {}", desc)
            }

            println!("\tmodes:");

            for mode in info.modes {
                println!("\t\t{}", mode);
            }
        }
    }

    // Seats
    println!();

    for (index, seat) in compositor_info.seats.iter().enumerate() {
        if let Some(info) = compositor_info.protocols.seats.info(seat) {
            print!("Seat #{}", index);

            if let Some(name) = &info.name {
                // Some compositors may have a seat with no name.
                if !name.is_empty() {
                    print!(" name: {}", name);
                }
            }

            print!(" (");

            if !info.has_keyboard && !info.has_pointer && !info.has_touch {
                print!("none");
            } else {
                if info.has_keyboard {
                    print!("keyboard");

                    if info.has_pointer || info.has_touch {
                        print!(", ");
                    }
                }

                if info.has_pointer {
                    print!("pointer");

                    if info.has_touch {
                        print!(", ");
                    }
                }

                if info.has_touch {
                    print!("touch");
                }
            }

            println!(")");
        }
    }

    // wl_shm
    println!();

    let formats = compositor_info.protocols.shm.formats();

    if !formats.is_empty() {
        println!("wl_shm formats:");

        for format in formats {
            println!("{:?} (0x{:x})", format, *format as u32);
        }
    }
}

struct CompositorInfo {
    globals: Vec<Global>,
    seats: Vec<wl_seat::WlSeat>,
    outputs: Vec<wl_output::WlOutput>,
    protocols: ProtocolStates,
}

struct ProtocolStates {
    shm: ShmState,
    seats: SeatState,
    outputs: OutputState,
    registry: RegistryState,
}

struct Global {
    name: u32,
    version: u32,
    interface: String,
}

delegate_registry!(CompositorInfo: [
    // Send registry events to ourselves.
    // Further explanation is provided in the RegistryHandler implementation.
    CompositorInfo,

    // ShmState needs to be told about it's wl_shm global.
    ShmState,

    // SeatState needs to be told about all the wl_seat globals.
    SeatState,

    // OutputState needs to be told about all the wl_output globals.
    OutputState,
]);

impl ProvidesRegistryState for CompositorInfo {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.protocols.registry
    }
}

// This is a bit weird: We implement RegistryHandler for CompositorInfo to delegate registry handling to
// ourselves for the purpose of getting the list of globals.
impl RegistryHandler<Self> for CompositorInfo {
    fn new_global(
        data: &mut Self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        name: u32,
        interface: &str,
        version: u32,
    ) {
        data.globals.push(Global { name, version, interface: interface.to_owned() });
    }

    fn remove_global(
        _data: &mut Self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        _name: u32,
    ) {
        // Not used in example
    }
}

// Required ShmState trait implementations
impl ShmHandler for CompositorInfo {
    fn shm_state(&mut self) -> &mut ShmState {
        &mut self.protocols.shm
    }
}

// delegate shm handling to ShmState.
delegate_shm!(CompositorInfo);

// Required SeatState trait implementations
impl SeatHandler for CompositorInfo {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.protocols.seats
    }

    fn new_seat(
        &mut self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
    ) {
        self.seats.push(seat);
    }

    fn new_capability(
        &mut self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
        // Seat info is obtained using SeatState
    }

    fn remove_capability(
        &mut self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
        // Seat info is obtained using SeatState
    }

    fn remove_seat(
        &mut self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
        // Not needed for example.
    }
}

delegate_seat!(CompositorInfo);

// Required OutputState trait implementations
impl OutputHandler for CompositorInfo {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.protocols.outputs
    }

    fn new_output(
        &mut self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.outputs.push(output);
    }

    fn update_output(
        &mut self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        // Seat info is obtained using OutputState
    }

    fn output_destroyed(
        &mut self,
        _conn: &mut ConnectionHandle,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        // Not needed for example.
    }
}

delegate_output!(CompositorInfo);
