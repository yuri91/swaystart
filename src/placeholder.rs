use calloop::channel::{channel, Event, Sender};
use std::mem::ManuallyDrop;
use std::thread::{spawn, JoinHandle};
use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_registry, delegate_shm, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
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
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

struct Client {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    xdg_shell: XdgShell,
    queue_handle: QueueHandle<Client>,

    exit: bool,
    exit_on_idle: bool,
    pool: SlotPool,
    windows: Vec<Window>,
}

enum ClientMsg {
    NewWindow { title: String, app_id: String },
    ExitOnIdle,
}

pub struct ClientHandle {
    chan: ManuallyDrop<Sender<ClientMsg>>,
    thread: ManuallyDrop<JoinHandle<()>>,
    wait: bool,
}
impl ClientHandle {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        let handle = spawn(move || {
            let mut event_loop: EventLoop<Client> =
                EventLoop::try_new().expect("Failed to initialize the event loop!");
            let loop_handle = event_loop.handle();
            loop_handle
                .insert_source(receiver, |ev, _, client| match ev {
                    Event::Closed => {
                        client.exit = true;
                    }
                    Event::Msg(ClientMsg::NewWindow { title, app_id }) => {
                        client.new_window(&title, &app_id);
                    }
                    Event::Msg(ClientMsg::ExitOnIdle) => {
                        client.exit_on_idle = true;
                    }
                })
                .expect("failed to register channel source");

            let mut client = Client::new(loop_handle);

            loop {
                event_loop
                    .dispatch(Duration::from_millis(16), &mut client)
                    .unwrap();

                if client.exit {
                    break;
                }
                if client.exit_on_idle && client.windows.is_empty() {
                    break;
                }
            }
        });
        Self {
            chan: ManuallyDrop::new(sender),
            thread: ManuallyDrop::new(handle),
            wait: false,
        }
    }
    pub fn new_window(&self, title: &str, app_id: &str) {
        self.chan
            .send(ClientMsg::NewWindow {
                title: title.to_owned(),
                app_id: app_id.to_owned(),
            })
            .expect("failed to send");
    }
    pub fn wait_until_idle(mut self) {
        self.chan.send(ClientMsg::ExitOnIdle).expect("failed to send");
        self.wait = true;
    }
}
impl Drop for ClientHandle {
    fn drop(&mut self) {
        unsafe {
            if !self.wait {
                let c = ManuallyDrop::take(&mut self.chan);
                drop(c);
            }
            let t = ManuallyDrop::take(&mut self.thread);
            let _ = t.join();
        }
    }
}

impl Client {
    fn new(loop_handle: LoopHandle<Client>) -> Self {
        // All Wayland apps start by connecting the compositor (server).
        let conn = Connection::connect_to_env().unwrap();

        // Enumerate the list of globals to get the protocols the server implements.
        let (globals, event_queue) = registry_queue_init(&conn).unwrap();
        let queue_handle = event_queue.handle();
        WaylandSource::new(conn.clone(), event_queue)
            .insert(loop_handle)
            .unwrap();

        // The compositor (not to be confused with the server which is commonly called the compositor) allows
        // configuring surfaces to be presented.
        let compositor =
            CompositorState::bind(&globals, &queue_handle).expect("wl_compositor not available");
        // For desktop platforms, the XDG shell is the standard protocol for creating desktop windows.
        let xdg_shell =
            XdgShell::bind(&globals, &queue_handle).expect("xdg shell is not available");
        // Since we are not using the GPU in this example, we use wl_shm to allow software rendering to a buffer
        // we share with the compositor process.
        let shm = Shm::bind(&globals, &queue_handle).expect("wl shm is not available.");

        // We don't know how large the window will be yet, so lets assume the minimum size we suggested for the
        // initial memory allocation.
        let pool = SlotPool::new(256 * 256 * 4, &shm).expect("Failed to create pool");

        Self {
            // Seats and outputs may be hotplugged at runtime, therefore we need to setup a registry state to
            // listen for seats and outputs.
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &queue_handle),
            shm,
            compositor,
            xdg_shell,
            queue_handle,

            exit: false,
            exit_on_idle: false,
            pool,
            windows: vec![],
        }
    }
    fn new_window(&mut self, title: &str, app_id: &str) {
        // A window is created from a surface.
        let surface = self.compositor.create_surface(&self.queue_handle);
        // And then we can create the window.
        let window = self.xdg_shell.create_window(
            surface,
            WindowDecorations::RequestServer,
            &self.queue_handle,
        );
        // Configure the window, this may include hints to the compositor about the desired minimum size of the
        // window, app id for WM identification, the window title, etc.
        window.set_title(title);
        // GitHub does not let projects use the `org.github` domain but the `io.github` domain is fine.
        window.set_app_id(app_id);
        window.set_min_size(Some((256, 256)));

        // In order for the window to be mapped, we need to perform an initial commit with no attached buffer.
        // For more info, see WaylandSurface::commit
        //
        // The compositor will respond with an initial configure that we can then use to present to the window with
        // the correct options.
        window.commit();

        self.windows.push(window);
    }
    pub fn draw(
        &mut self,
        _conn: &Connection,
        queue_handle: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        width: u32,
        height: u32,
    ) {
        let stride = width as i32 * 4;

        let buffer = self
            .pool
            .create_buffer(
                width as i32,
                height as i32,
                stride,
                wl_shm::Format::Argb8888,
            )
            .expect("create buffer")
            .0;

        // Request our next frame
        surface.frame(queue_handle, surface.clone());

        // Attach and commit to present.
        buffer.attach_to(surface).expect("buffer attach");
        surface.commit();
    }
}

impl CompositorHandler for Client {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
        // Not needed for this example.
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
        // Not needed for this example.
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        // Not needed for this example.
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        // Not needed for this example.
    }
}

impl OutputHandler for Client {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for Client {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, w: &Window) {
        if let Some(idx) = self.windows.iter().position(|ow| ow == w) {
            self.windows.swap_remove(idx);
        }
    }

    fn configure(
        &mut self,
        conn: &Connection,
        queue_handle: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let width = configure.new_size.0.map(|v| v.get()).unwrap_or(256);
        let height = configure.new_size.1.map(|v| v.get()).unwrap_or(256);
        self.draw(conn, queue_handle, window.wl_surface(), width, height);
    }
}

impl ShmHandler for Client {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(Client);
delegate_output!(Client);
delegate_shm!(Client);

delegate_xdg_shell!(Client);
delegate_xdg_window!(Client);

delegate_registry!(Client);

impl ProvidesRegistryState for Client {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState,];
}
