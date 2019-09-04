mod jack;

use crate::jack::JackInterface;


// FIXME: Replace (e)println with RT-safe logging everywhere

fn main() {
    // Set up the audio work
    let jack_interface = JackInterface::new();

    // TODO: Do graphics stuff
    loop {
        std::thread::sleep_ms(1000);
        assert!(jack_interface.is_alive(), "Audio thread has died");
        eprintln!("Jack clock at end of last processed frame: {:?} µs",
                  jack_interface.next_time());
    }
}