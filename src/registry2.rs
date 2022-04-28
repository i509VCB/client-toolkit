use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use wayland_backend::{
    client::{Handle, ObjectData, ObjectId, WaylandError},
    protocol::Message,
};
use wayland_client::{
    protocol::{wl_display, wl_registry},
    ConnectionHandle, Proxy, Connection,
};

#[derive(Debug)]
pub struct Registry {
    inner: Arc<Mutex<RegistryInner>>,
    wl_registry: wl_registry::WlRegistry,
}

impl Registry {
    // TODO: Should this function have a non-blocking alternative?
    pub fn init(conn: &mut Connection) -> Result<Registry, WaylandError> {
        let mut handle = conn.handle();
        let display = handle.display();
        let inner = Arc::new(Mutex::new(RegistryInner { globals: HashMap::new() }));
        let registry_id = handle.send_request(
            &display,
            wl_display::Request::GetRegistry {},
            Some(Arc::new(RegistryData { inner: inner.clone() })),
        )
        // TODO: The display must always be alive?
        .unwrap();
        let wl_registry = wl_registry::WlRegistry::from_id(&mut handle, registry_id).unwrap();

        // Perform a roundtrip to initialize the registry.
        conn.roundtrip()?;

        Ok(Registry { inner, wl_registry })
    }

    pub fn globals(&self) -> impl Iterator<Item = Global> {
        let inner = self.inner.lock().unwrap();

        inner
            .globals
            .clone()
            .into_values()
    }

    pub fn wl_registry(&self) -> &wl_registry::WlRegistry {
        &self.wl_registry
    }
}

#[derive(Debug, Clone)]
pub struct Global {
    pub name: u32,
    pub interface: String,
    pub version: u32,
}

#[derive(Debug)]
struct RegistryInner {
    globals: HashMap<u32, Global>,
}

struct RegistryData {
    inner: Arc<Mutex<RegistryInner>>,
}

impl ObjectData for RegistryData {
    fn event(
        self: Arc<Self>,
        handle: &mut Handle,
        msg: Message<ObjectId>,
    ) -> Option<Arc<dyn ObjectData>> {
        let mut conn = ConnectionHandle::from(handle);
        let (_registry, event) = wl_registry::WlRegistry::parse_event(&mut conn, msg)
            .expect("invalid registry protocol object");

        let mut inner = self.inner.lock().unwrap();

        match event {
            wl_registry::Event::Global { name, interface, version } => {
                inner.globals.insert(name, Global {
                    name,
                    interface,
                    version,
                });
            },

            wl_registry::Event::GlobalRemove { name } => {
                inner.globals.remove(&name);
            },

            _ => unreachable!(),
        }

        None
    }

    fn destroyed(&self, _object: ObjectId) {}
}
