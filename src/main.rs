use ::jack::{
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
    Time,
};

use std::{
    panic,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};


const CLIENT_NAME: &'static str = "dbmeter";
const PORT_NAME: &'static str = "in";


// FIXME: Replace (e)println with RT-safe logging

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
    println!("initial frame time is {} µs.", ::jack::get_time());

    // Register an audio input
    let input_port =
        client.register_port(PORT_NAME, AudioIn)
              .expect("Failed to register input port");

    // Start the audio thread
    let jack_interface = JackHandler::new(input_port);
    let _async_client = client.activate_async(
        jack_interface.clone(),
        jack_interface.clone(),
    );

    // TODO: Do graphics stuff
    loop {
        std::thread::sleep_ms(1000);
        assert!(jack_interface.is_alive(), "Audio thread has died");
        eprintln!("Jack clock at end of last processed frame: {:?} µs",
                  jack_interface.next_time());
    }
}


// This shared struct received the callbacks from JACK and will be used by other
// threads to query the status of the audio thread.
struct JackState {
    // Access to our input audio port
    input_port: Port<AudioIn>,

    // Truth that the audio thread is alive
    alive: AtomicBool,

    // Jack clock timestamp as of the end of the last processed frame
    next_time: AtomicU64,
}

// We need to Arc it because it's shared between thread, obviously
#[derive(Clone)]
struct JackHandler(Arc<JackState>);

// NOTE: Every public interface function other than is_alive() should feature a
//       debug assertion that the audio thread is still alive, as a debugging
//       aid for ill-behaved clients that forget to check it.
impl JackHandler {
    // Prepare for communication between audio thread and the rest of the world
    fn new(input_port: Port<AudioIn>) -> Self {
        Self(Arc::new(JackState {
            input_port,
            alive: AtomicBool::new(true),
            next_time: AtomicU64::new(::jack::get_time())
        }))
    }

    // Check out the JACK input port, we don't really want to hide it
    pub fn input_port(&self) -> &Port<AudioIn> {
        debug_assert!(self.is_alive(), "Audio thread has died.");
        &self.0.input_port
    }

    // Check if the audio thread is still alive, please do this periodically
    pub fn is_alive(&self) -> bool {
        self.0.alive.load(Ordering::Relaxed)
    }

    // Mark the audio thread as dead
    fn mark_dead(&self) {
        self.0.alive.store(false, Ordering::Relaxed);
    }

    // Query JACK clock as of the end of the last processed audio frame
    //
    // Provides an Acquire barrier so that you can synchronize with any write
    // made during the process() callback. Should be called first by clients.
    //
    pub fn next_time(&self) -> Time {
        debug_assert!(self.is_alive(), "Audio thread has died.");
        self.0.next_time.load(Ordering::Acquire)
    }

    // Update the JACK clock to account for newly processed frames
    //
    // Provides a Release barrier so that clients can synchronize with writes
    // made during the process() callback. Should be called last by process().
    //
    fn update_time(&self, scope: &ProcessScope) {
        let next_time =
            scope.cycle_times()
                 .expect("JACK lib does not seem to support cycle timing")
                 .next_usecs;
        self.0.next_time.store(next_time, Ordering::Release);
    }

    // JACK callback wrapper that makes sure the audio thread honors its own
    // liveness signal, prevents panic-induced UB, and translates panics or
    // voluntary exits into implicit setting of the death signal.
    fn callback_guard<F>(&self, callback: F) -> Control
        where F: FnMut() -> Control + panic::UnwindSafe
    {
        if !self.is_alive() { return Control::Quit; }
        let result = panic::catch_unwind(callback);
        // FIXME: Store error somewhere so it can be processed, something based
        //        on AtomicPtr could do the trick and be async signal safe.
        let output = result.unwrap_or(Control::Quit);
        if output == Control::Quit { self.mark_dead(); }
        output
    }
}

impl ProcessHandler for JackHandler {
    // Hook to process incoming audio data
    fn process(&mut self, _: &Client, scope: &ProcessScope) -> Control {
        self.callback_guard(|| {
            // Fetch input frames
            let input = self.input_port().as_slice(scope);

            // FIXME: Do some actual audio processing
            std::mem::drop(input);

            // Update client view of the JACK clock
            self.update_time(scope);
            Control::Continue
        })
    }
}

impl NotificationHandler for JackHandler {
    // Hook to do initialization before an audio thread starts
    fn thread_init(&self, _: &Client) {
        self.callback_guard(|| {
            println!("Audio thread {:?} is ready.",
                     std::thread::current().id());
            Control::Continue
        });
    }

    // Hook to handle JACK server shutting down our audio thread
    //
    // WARNING: In the JACK devs' words, this is like a POSIX signal handler. So
    //          many libc functions cannot be called, and garbage data can be
    //          seen. This function actually shouldn't be marked as safe.
    //
    fn shutdown(&mut self, status: ClientStatus, reason: &str) {
        self.callback_guard(|| {
            // FIXME: Find a way to communicate "status" and "reason" without
            //        calling signal-unsafe functions like malloc or println,
            //        maybe RT-safe logging will also save us here?
            eprintln!("JACK is shutting us down with status {:?} ({})",
                      status, reason);
            Control::Quit
        });
    }

    // Hook to handle JACK going in and out of "freewheel" mode, where audio
    // frames are just dumped in as quickly as possible. To support it, we need
    // to take the following precautions:
    //
    // 1. Commit to either the JACK clock or system clock, and never mix them
    // 2. Never buffer data based on a system time interval, as that would
    //    require storing an unbounded amount of audio frames. This is an
    //    argument in favor of choosing the JACK clock.
    //
    fn freewheel(&mut self, _: &Client, is_freewheel_enabled: bool) {
        self.callback_guard(|| {
            if is_freewheel_enabled {
                print!("Entering freewheeling mode. ");
                println!("JACK clock may go much faster than real time!");
            } else {
                print!("Leaving freewheeling mode. ");
                println!("JACK clock will go back in sync with real time.");
            }
            Control::Continue
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
            eprintln!();
            eprintln!("Audio data was dropped. This should never happen!");
            eprintln!("Either JACK is misconfigured, or our code is wrong.");
            eprintln!("If other JACK apps work for you, please file a bug.");
            Control::Continue
        })
    }

    // NOTE: We probably don't need to monitor client registration, port
    //       registration/renaming/connection, and graph reordering.
    //
    //       The JACK docs also tell us that as a single-input application, we
    //       do not need a latency update callback.
}