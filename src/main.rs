use jack::{
    AudioIn,
    Client,
    ClientOptions,
    ClientStatus,
    Control,
    Frames,
    NotificationHandler,
    Port,
    ProcessHandler,
    ProcessScope,
};

use std::{
    panic,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};


const CLIENT_NAME: &'static str = "dbmeter";
const PORT_NAME: &'static str = "in";


fn main() {
    // Create a JACK client
    let (client, mut status) =
        Client::new(CLIENT_NAME, ClientOptions::empty())
               .expect("Failed to open a JACK client");

    // Cross-check initial client status
    let bad_status_mask = ClientStatus::FAILURE
                          | ClientStatus::INVALID_OPTION
                          | ClientStatus::SERVER_FAILED
                          | ClientStatus::SERVER_ERROR
                          | ClientStatus::NO_SUCH_CLIENT
                          | ClientStatus::LOAD_FAILURE
                          | ClientStatus::INIT_FAILURE
                          | ClientStatus::SHM_FAILURE
                          | ClientStatus::VERSION_ERROR
                          | ClientStatus::BACKEND_ERROR
                          | ClientStatus::CLIENT_ZOMBIE;
    assert_eq!(status & bad_status_mask, ClientStatus::empty(),
               "Bad client initialization status");
    status.remove(bad_status_mask);
    let ignored_status_mask = ClientStatus::NAME_NOT_UNIQUE
                              | ClientStatus::SERVER_STARTED;
    status.remove(ignored_status_mask);
    assert_eq!(status, ClientStatus::empty(),
               "Unknown client initialization status");

    // Say hi to the user
    print!("Successfully initialized jack client \"{}\"! ", client.name());
    print!("Sample rate is {}, ", client.sample_rate());
    print!("buffer size is {}, ", client.buffer_size());
    println!("initial frame time is {} µs.", jack::get_time());

    // Register an audio input
    let input_port =
        client.register_port(PORT_NAME, AudioIn)
              .expect("Failed to register input port");

    // Start the audio thread
    let jack_interface = JackInterface::new(input_port);
    let _async_client = client.activate_async(
        jack_interface.clone(),
        jack_interface.clone(),
    );

    // TODO: Do graphics stuff
    std::thread::sleep_ms(1000);

    // NOTE: This is how we propagate panics from the audio thread to other
    //       threads, please run this assertion regularly.
    assert!(jack_interface.is_alive(), "Audio thread has died");
}


// This shared struct received the callbacks from JACK and will be used by other
// threads to query the status of the audio thread.
struct JackInterfaceState {
    // Access to our input audio port
    input_port: Port<AudioIn>,

    // Truth that the audio thread is alive
    alive: AtomicBool,
}

// We need to Arc it because it's shared between thread, obviously
#[derive(Clone)]
struct JackInterface(Arc<JackInterfaceState>);

impl JackInterface {
    // Prepare for communication between audio thread and the rest of the world
    fn new(input_port: Port<AudioIn>) -> Self {
        Self(Arc::new(JackInterfaceState {
            input_port,
            alive: AtomicBool::new(true),
        }))
    }

    // Check out the JACK input port, we don't really want to hide it
    fn input_port(&self) -> &Port<AudioIn> {
        &self.0.input_port
    }

    // Check if the audio thread is alive
    fn is_alive(&self) -> bool {
        self.0.alive.load(Ordering::Relaxed)
    }

    // Mark the audio thread as dead
    fn mark_dead(&self) {
        self.0.alive.store(false, Ordering::Relaxed);
    }

    // JACK callback wrapper that makes sure the audio thread honors its own
    // liveness signal, prevents panic-induced UB, and translates panics or
    // voluntary exits into implicit setting of the death signal.
    fn callback_guard<F>(&self, callback: F) -> Control
        where F: FnMut() -> Control + panic::UnwindSafe
    {
        if !self.is_alive() { return Control::Quit; }
        let result = panic::catch_unwind(callback);
        let output = result.unwrap_or(Control::Quit);
        if output == Control::Quit { self.mark_dead(); }
        output
    }
}

impl ProcessHandler for JackInterface {
    // Hook to process incoming audio data
    fn process(&mut self, _: &Client, scope: &ProcessScope) -> Control {
        self.callback_guard(|| {
            let input = self.input_port().as_slice(scope);

            // FIXME: Do some actual audio processing
            std::mem::drop(input);
            unimplemented!()
        })
    }
}

impl NotificationHandler for JackInterface {
    // Hook to do initialization before audio thread starts
    fn thread_init(&self, _: &Client) {
        self.callback_guard(|| {
            println!("Audio thread is ready.");
            Control::Continue
        });
    }

    // Hook to handle JACK server shutting down our audio thread
    // WARNING: In the JACK devs' words, this is like a POSIX signal handler. So
    //          many libc functions cannot be called, and garbage data can be
    //          seen. This function actually shouldn't be marked as safe.
    fn shutdown(&mut self, _status: ClientStatus, _reason: &str) {
        self.callback_guard(|| {
            // FIXME: Find a way to communicate "status" and "reason" without
            //        calling signal-unsafe functions like malloc or println
            Control::Quit
        });
    }

    // Hook to handle JACK going in and out of "freewheel" mode, where audio
    // frames are just dumped in as quickly as possible
    fn freewheel(&mut self, _: &Client, is_freewheel_enabled: bool) {
        self.callback_guard(|| {
            // FIXME: Support freewheel mode properly
            eprintln!("Freewheel mode new status: {}.", is_freewheel_enabled);
            unimplemented!()
        });
    }

    // Hook to handle JACK buffer size changes
    fn buffer_size(&mut self, _: &Client, size: Frames) -> Control {
        self.callback_guard(|| {
            // FIXME: Support buffer size changes properly
            eprintln!("Buffer size is now: {}", size);
            unimplemented!()
        })
    }

    // Hook to handle JACK sample rate changes
    fn sample_rate(&mut self, _: &Client, srate: Frames) -> Control {
        self.callback_guard(|| {
            // FIXME: Support sample rate changes properly
            eprintln!("Sample rate is now: {}", srate);
            unimplemented!()
        })
    }

    // Hook to handle audio data loss due to buffer under- or over-run
    fn xrun(&mut self, _: &Client) -> Control {
        self.callback_guard(|| {
            // FIXME: Support xrun notifications properly
            eprintln!("An x-run has occured, OH NOES WE FAILED REALTIME!!!");
            unimplemented!()
        })
    }
}