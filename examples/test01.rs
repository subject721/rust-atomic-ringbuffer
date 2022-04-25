use std::time::Instant;
use atomic_ring_buffer::create_ring_buffer;

pub fn main() {
    let (mut buffer_writer, mut buffer_reader) = create_ring_buffer::<String>(8);

    let num_messages = 64;

    let producer_thread = std::thread::spawn(move || {
        for idx in 0..num_messages {
            let msg = String::from(format!("Message {}", idx));

            let tstart = Instant::now();

            let mut result = buffer_writer.try_write(msg);

            let tend = Instant::now();

            println!("write took {} ns", tend.duration_since(tstart).as_nanos() as u64);

            while result.is_err() {
                std::thread::sleep(std::time::Duration::from_millis(10));

                result = buffer_writer.try_write(result.err().unwrap());
            }
        }
    });

    let consumer_thread = std::thread::spawn(move || {
        let mut num_received_messages = 0;

        while num_received_messages < num_messages {
            let received_msg = buffer_reader.try_read();

            match received_msg {
                Some(v) => {
                    println!("Received:  {}", v);

                    num_received_messages += 1;
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
    });

    producer_thread
        .join()
        .expect("Could not join producer thread");
    consumer_thread
        .join()
        .expect("Could not join consumer thread");
}
