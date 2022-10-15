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
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

type SensorHandle = std::thread::JoinHandle<Result<(), simple_error::SimpleError>>;

#[derive(Deserialize, Clone)]
struct Config {
    sensors: HashMap<String, AgentSensorConfig>,
}

impl std::fmt::Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for config in &self.sensors {
            write!(f, "\n{}:\n{}", config.0, config.1).expect("Failed to fprmat config.");
        }
        Ok(())
    }
}

#[derive(Deserialize, Clone)]
struct AgentSensorConfig {
    sensor: String,
    sample_interval: u64,
    details: Option<toml::Value>,
}

impl AgentSensorConfig {
    fn interval(&self) -> Duration {
        Duration::from_secs(self.sample_interval)
    }
}

impl std::fmt::Display for AgentSensorConfig {
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
    fn new(name: String, config: AgentSensorConfig) -> AgentSensor {
        let lib = Library::new(config.sensor.as_str())
            .expect(format!("Missing library {}", config.sensor).as_str());
        let details_config = toml::to_string(&config.details).unwrap_or("".to_string());
        let sensor = unsafe {
            let initialize: Symbol<unsafe extern "C" fn(&str) -> *mut dyn Sensor> = lib
                .get(b"initialize")
                .expect(format!("Failed to initialize {}", config.sensor).as_str());
            Box::from_raw(initialize(details_config.as_str()))
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

fn start_publisher(receivers: Vec<Receiver<SensorOutput>>) -> SensorHandle {
    std::thread::Builder::new()
        .name("publisher".to_string())
        .spawn(move || {
            let mut alive_receivers: Vec<&Receiver<SensorOutput>> = receivers.iter().collect();
            while !alive_receivers.is_empty() {
                debug!("Sleeping between publishes");
                std::thread::sleep(Duration::from_secs(1));
                alive_receivers.iter().fold(
                    Vec::<&Receiver<SensorOutput>>::with_capacity(alive_receivers.len()),
                    |mut list, rx| {
                        let mut read = true;
                        let mut closed = false;
                        while !closed && read {
                            match rx.try_recv() {
                                Ok(sample) => {
                                    debug!("Read sample.");
                                    match sample {
                                        SensorOutput::Process { pid, stat } => {
                                            println!("Process {} {:?}", pid, stat)
                                        }
                                        SensorOutput::MountPoint { name, size, free } => println!(
                                            "Mount {}: {} %",
                                            name,
                                            ((size - free) as f64 / size as f64) * 100.0
                                        ),
                                    };
                                }
                                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                    error!("Channel closed.");
                                    closed = true;
                                }
                                Err(_) => {
                                    debug!("Empty Channel.");
                                    list.push(rx);
                                    read = false;
                                }
                            };
                        }
                        if !closed {
                            debug!("Keeping receiver.");
                            list.push(rx);
                        }
                        list
                    },
                );
            }
            Ok(())
        })
        .expect("Failed to start the publisher thread")
}

fn start_sensor(name: String, config: AgentSensorConfig, tx: Sender<SensorOutput>) -> SensorHandle {
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let sensor = AgentSensor::new(name, config);
            loop {
                sensor.sample(&tx);
                std::thread::sleep(sensor.interval);
            }
        })
        .expect(format!("Failed to start the sensor thread.").as_str())
}

fn main() {
    env_logger::init();

    let config: Config = toml::from_slice(&std::fs::read("spotr_agent.toml").unwrap()).unwrap();
    info!("Config:");
    info!("{}", config);

    info!("spotr-agent starting");
    let mut receivers = vec![];
    let mut sensors = vec![];
    for sensor_config in config.sensors {
        let (tx, rx): (Sender<SensorOutput>, Receiver<SensorOutput>) = std::sync::mpsc::channel();
        receivers.push(rx);
        info!("Starting {}", sensor_config.0);
        sensors.push(start_sensor(sensor_config.0, sensor_config.1, tx));
    }
    let publisher = start_publisher(receivers);

    info!("spotr-agent started");

    publisher.join().expect("Failed to join publisher thread");
    for sensor in sensors {
        sensor.join().expect("Failed to join sensor thread");
    }

    info!("spotr-agent exiting");
}
