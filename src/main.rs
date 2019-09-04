mod jack;
mod meters;

use crate::jack::JackInterface;


// Just a few typedefs to clarify things
pub type Sample = f32;
pub type Decibel = f32;

// FIXME: Replace (e)println with RT-safe logging everywhere
fn main() {
    // Set up the audio work
    let jack_interface = JackInterface::new();

    // TODO: Display Real Pretty graphics, not console prints
    loop {
        std::thread::sleep_ms(100);
        assert!(jack_interface.is_alive(), "Audio thread has died");
        eprintln!("Audio peak during last period: {} dBFS",
                  jack_interface.read_and_reset_peak());
        eprintln!("Current audio loudness: {} VUFS",
                  jack_interface.read_loudness());
        eprintln!("Jack clock at end of last processed frame: {:?} µs",
                  jack_interface.next_time());
    }
}