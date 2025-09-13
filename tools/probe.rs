use std::{env, fs, thread};

fn main() {
    let mut args = env::args().skip(1);
    let mut hosts = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "-f" {
            if let Some(file) = args.next() {
                if let Ok(data) = fs::read_to_string(&file) {
                    hosts.extend(data.lines().filter(|l| !l.is_empty()).map(str::to_string));
                }
            }
        } else {
            hosts.push(arg);
        }
    }
    if hosts.is_empty() {
        eprintln!("usage: probe [-f HOSTFILE] HOST[:PORT] ...");
        return;
    }
    let handles: Vec<_> = hosts
        .into_iter()
        .map(|h| {
            thread::spawn(move || {
                let url = format!("http://{}/metrics", h);
                match ureq::get(&url).call() {
                    Ok(resp) => match resp.into_string() {
                        Ok(body) => {
                            println!("# {}", h);
                            print!("{}", body);
                        }
                        Err(e) => eprintln!("{}: {}", h, e),
                    },
                    Err(e) => eprintln!("{}: {}", h, e),
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
}
