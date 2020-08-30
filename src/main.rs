extern crate env_logger;
extern crate libloading;
extern crate log;
extern crate serde;
extern crate serde_derive;
extern crate simple_error;
extern crate toml;

use libloading::{Library, Symbol};
use log::{debug, error, info, trace, warn};
use serde_derive::Deserialize;
use spotr_sensing::{Sensor, SensorOutput};
use std::any::Any;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

#[derive(Deserialize, Clone)]
struct Config {
    process_sensor: SensorConfig,
}

impl std::fmt::Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "\nprocess_sensor:\n{}", self.process_sensor)
    }
}

#[derive(Deserialize, Clone)]
struct SensorConfig {
    sensor: String,
    sample_interval: u64,
}

impl SensorConfig {
    fn interval(&self) -> Duration {
        Duration::from_secs(self.sample_interval)
    }
}

impl std::fmt::Display for SensorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "sensor: {}\nsample_interval: {:?}",
            self.sensor, self.sample_interval
        )
    }
}

struct AgentSensor {
    name: String,
    lib: Library,
    interval: Duration,
    sensor: Box<dyn Sensor>,
}

impl AgentSensor {
    fn new(name: String, config: SensorConfig) -> AgentSensor {
        let lib = Library::new(config.sensor.as_str())
            .expect(format!("Missing library {}", config.sensor).as_str());
        let sensor = unsafe {
            let initialize: Symbol<unsafe extern "C" fn() -> *mut dyn Sensor> = lib
                .get(b"initialize")
                .expect(format!("Failed to initialize {}", config.sensor).as_str());
            Box::from_raw(initialize())
        };

        AgentSensor {
            name: name,
            lib: lib,
            interval: config.interval(),
            sensor: sensor,
        }
    }

    fn sample(&self, tx: &Sender<SensorOutput>) {
        info!("{} sampling", self.name);
        match self.sensor.sample() {
            Ok(samples) => {
                for sample in samples {
                    match tx.send(sample) {
                        Err(e) => error!("Failed to send sample: {}", e),
                        _ => {}
                    }
                }
            }
            Err(e) => error!("Sampling failed: {}", e),
        }
        info!("{} sampled", self.name);
    }
}

fn start_publisher(
    rx: Receiver<SensorOutput>,
) -> std::thread::JoinHandle<Result<(), simple_error::SimpleError>> {
    std::thread::Builder::new()
        .name("publisher".to_string())
        .spawn(move || {
            for sample in rx.iter() {
                match sample {
                    SensorOutput::Process { pid } => println!("Process {}", pid),
                };
            }
            Ok(())
        })
        .expect("Failed to start the publisher thread")
}

fn start_sensor(
    name: &'static str,
    config: SensorConfig,
    tx: Sender<SensorOutput>,
) -> std::thread::JoinHandle<Result<(), simple_error::SimpleError>> {
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let sensor = AgentSensor::new(name.to_string(), config);
            loop {
                sensor.sample(&tx);
                std::thread::sleep(sensor.interval);
            }
        })
        .expect(format!("Failed to start the sensor thread for {}", name).as_str())
}

fn main() {
    env_logger::init();

    let config: Config = toml::from_slice(&std::fs::read("spotr_agent.toml").unwrap()).unwrap();
    info!("Config:");
    info!("{}", config);

    info!("spotr-agent starting");
    let (tx, rx): (Sender<SensorOutput>, Receiver<SensorOutput>) = std::sync::mpsc::channel();
    let sensors = vec!(start_sensor("process_sensor", config.process_sensor, tx));
    let publisher = start_publisher(rx);

    info!("spotr-agent started");

    publisher.join().expect("Failed to join publisher thread");
    for sensor in sensors {
        sensor.join().expect("Failed to join sensor thread");
    }

    info!("spotr-agent exiting");
}
