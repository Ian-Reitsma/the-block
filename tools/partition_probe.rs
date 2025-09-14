use std::env;

fn main() {
    let host = env::args().nth(1).expect("usage: partition_probe HOST[:PORT]");
    let url = format!("http://{}/metrics", host);
    match ureq::get(&url).call() {
        Ok(resp) => {
            if let Ok(body) = resp.into_string() {
                for line in body.lines() {
                    if line.starts_with("partition_events_total")
                        || line.starts_with("partition_recover_blocks")
                    {
                        println!("{}", line);
                    }
                }
            }
        }
        Err(e) => eprintln!("{}: {}", host, e),
    }
}
