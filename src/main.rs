use chrono::{DateTime, Utc};
use gethostname;
use serde_json;
use std::{thread, time::Duration};
fn seconds_since_last_input() -> Result<f64, winsafe::co::ERROR> {
    let last_input = winsafe::GetLastInputInfo()?.dwTime as u64;
    let tick_count = winsafe::GetTickCount64();
    Ok((tick_count - last_input) as f64 / 1000.0)
}
use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value_t = 180.0)]
    timeout: f64,
    #[arg(long, default_value_t = 1.0)]
    poll_time: f64,

    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(short, long, default_value_t = 5600)]
    port: u16,

    #[arg(short, long, default_value_t = false)]
    debug: bool,
}

#[derive(Debug)]
struct Settings {
    timeout: f64,
    poll_time: f64,
}

impl Settings {
    fn new(timeout: f64, poll_time: f64) -> Self {
        if timeout < poll_time {
            panic!("timeout must be greater than or equal to poll_time");
        }
        Settings { timeout, poll_time }
    }
}

#[derive(Debug)]
struct AFKWatcher {
    settings: Settings,
    client: aw_client_rust::blocking::AwClient,
    bucket_name: String,
}

impl AFKWatcher {
    fn new(settings: Settings, host: String, port: u16) -> Self {
        let hostname = gethostname::gethostname()
            .into_string()
            .expect("Failed to get hostname");
        let client = aw_client_rust::blocking::AwClient::new(&host, port, "aw-watcher-afk-rs")
            .expect("Failed to create a client");
        let bucket_name = format!("aw-watcher-afk-rs_{}", hostname);

        Self {
            settings,
            client,
            bucket_name,
        }
    }

    fn ping(&self, afk: bool, timestamp: DateTime<Utc>) {
        let mut data = serde_json::Map::new();
        let status = if afk { "afk" } else { "not-afk" };
        data.insert(
            "status".to_string(),
            serde_json::Value::String(status.to_string()),
        );
        let event = aw_client_rust::Event {
            id: None,
            timestamp,
            data,
            duration: chrono::Duration::seconds(0),
        };
        let pulsetime = self.settings.timeout + self.settings.poll_time + 1.0;

        if let Err(e) = self.client.heartbeat(&self.bucket_name, &event, pulsetime) {
            eprintln!("Failed to send heartbeat: {}", e);
        }
    }

    fn run(&mut self) {
        println!("aw-watcher-afk-rs started");

        loop {
            match self
                .client
                .create_bucket_simple(&self.bucket_name, "afkstatus")
            {
                Ok(_) => break,
                Err(e) => {
                    eprintln!("Failed to create bucket: {}. Retrying...", e);
                    thread::sleep(Duration::from_millis(1000));
                }
            }
        }

        self.heartbeat_loop();
    }

    fn heartbeat_loop(&self) {
        let mut afk = false;
        let td1ms = chrono::Duration::milliseconds(1);

        loop {
            thread::sleep(Duration::from_secs_f64(self.settings.poll_time));

            let now = Utc::now();
            let seconds_since_input = match seconds_since_last_input() {
                Ok(time) => time,
                Err(e) => {
                    eprintln!("Failed to get last input time: {}", e);
                    continue;
                }
            };

            println!("Seconds since last input: {}", seconds_since_input);

            let should_change_afk_state = match (afk, seconds_since_input) {
                (true, time) if time < self.settings.timeout => {
                    println!("No longer AFK");
                    Some(false)
                }
                (false, time) if time >= self.settings.timeout => {
                    println!("Became AFK");
                    Some(true)
                }
                _ => None
            };

            self.ping(afk, now);
            if let Some(new_afk_state) = should_change_afk_state {
                afk = new_afk_state;
                self.ping(afk, now + td1ms);
            }
        }
    }
}

fn main() {
    let args = Args::parse();
    let settings = Settings::new(args.timeout, args.poll_time);
    let mut watcher = AFKWatcher::new(settings, args.host, args.port);

    watcher.run();
}
