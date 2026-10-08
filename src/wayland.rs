use wayland_client::Proxy;
use std::sync::{Arc, Mutex, OnceLock};
use std::collections::HashMap;
use wayland_client::{protocol::wl_registry, Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use crate::WindowInfo;

pub static WINDOWS_STATE: OnceLock<Arc<Mutex<HashMap<u32, WindowInfo>>>> = OnceLock::new();

struct AppState {
    windows: Arc<Mutex<HashMap<u32, WindowInfo>>>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        _state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, .. } = event {
            if interface == "ext_foreign_toplevel_list_v1" {
                registry.bind::<ExtForeignToplevelListV1, _, _>(name, 1, qh, ());
            }
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            let mut wins = state.windows.lock().unwrap();
            wins.insert(toplevel.id().protocol_id(), WindowInfo {
                id: toplevel.id().protocol_id().to_string(),
                app_id: String::new(),
                title: String::new(),
            });
        }
    }
    wayland_client::event_created_child!(AppState, ExtForeignToplevelListV1, [
        0 => (ExtForeignToplevelHandleV1, ())
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = handle.id().protocol_id();
        let mut wins = state.windows.lock().unwrap();
        if let Some(win) = wins.get_mut(&id) {
            match event {
                ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                    win.app_id = app_id;
                }
                ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                    win.title = title;
                }
                ext_foreign_toplevel_handle_v1::Event::Closed => {
                    wins.remove(&id);
                }
                _ => {}
            }
        }
    }
}

pub fn spawn_listener() {
    let state_arc = Arc::new(Mutex::new(HashMap::new()));
    WINDOWS_STATE.set(state_arc.clone()).unwrap();

    std::thread::spawn(move || {
        if let Ok(conn) = Connection::connect_to_env() {
            let mut event_queue = conn.new_event_queue();
            let qh = event_queue.handle();
            let display = conn.display();
            display.get_registry(&qh, ());

            let mut state = AppState {
                windows: state_arc,
            };

            // First roundtrips to get globals and existing windows
            let _ = event_queue.roundtrip(&mut state);
            let _ = event_queue.roundtrip(&mut state);

            // Infinite loop to keep listening for new/closed windows
            loop {
                if event_queue.blocking_dispatch(&mut state).is_err() {
                    break;
                }
            }
        }
    });
}
