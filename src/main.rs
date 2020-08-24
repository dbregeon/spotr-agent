extern crate libloading;
extern crate log;
extern crate env_logger;
extern crate serde;
extern crate serde_derive;
extern crate toml;
extern crate simple_error;

use libloading::{Library, Symbol};
use spotr_sensing::{Sensor, Process};
use log::{debug, info, warn};
use serde_derive::Deserialize;
use std::time::Duration;

#[derive(Deserialize, Clone)]
struct Config {
    process_sensor: SensorConfig
}

impl std::fmt::Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "\nprocess_sensor:\n{}", self.process_sensor)
    }
}

#[derive(Deserialize, Clone)]
struct SensorConfig {
    sensor: String,
    sample_interval: u64
}

impl SensorConfig {
    fn interval(&self) -> Duration {
        Duration::from_secs(self.sample_interval)
    }
}

impl std::fmt::Display for SensorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "sensor: {}\nsample_interval: {:?}", self.sensor, self.sample_interval)
    }
}

struct AgentSensor<T> {
    lib: Library,
    interval: Duration,
    sensor: Box<dyn Sensor<Item = T>>
}

impl <T> AgentSensor<T> {
    fn new(config: SensorConfig) -> Result<AgentSensor<T>, libloading::Error> {
        let lib = Library::new(config.sensor.as_str())?;
        let sensor = unsafe {
            let initialize: Symbol<unsafe extern fn() ->  *mut dyn Sensor<Item = T>> = lib.get(b"initialize")?;
            Box::from_raw(initialize())
        };

        Ok(AgentSensor::<T> {
            lib: lib,
            interval: config.interval(),
            sensor: sensor
        })
    }

    fn sample(&self) {
        info!("Sampling");
        self.sensor.sample().unwrap();
        info!("Sampled");
    }
}

struct Agent {
    sensors: Vec<std::thread::JoinHandle<Result<(), simple_error::SimpleError>>>
}

impl Agent {
    fn start(&mut self, config: Config) -> Result<(), simple_error::SimpleError> {
        self.sensors.push(start::<Process>("process_sensor", config.process_sensor)?);
        Ok(())
    }
}

fn start<T>(name: &'static str, config: SensorConfig) -> Result<std::thread::JoinHandle<Result<(), simple_error::SimpleError>>, simple_error::SimpleError> {
    match std::thread::Builder::new().name(name.to_string()).spawn(move || {
        let sensor_result: Result<AgentSensor<T>, libloading::Error> = AgentSensor::<T>::new(config);
        match sensor_result {
            Ok(sensor) => {
                loop {
                    sensor.sample();
                    std::thread::sleep(sensor.interval);
                }
            }
            Err(e) => Err(simple_error::SimpleError::new(format!("Failed to create the sensor {}: {}", name, e)))
        }
        
    }) {
        Ok(handler) => Ok(handler),
        Err(e) => Err(simple_error::SimpleError::new(format!("Failed to start the sensor thread for {}: {}", name, e)))
    }
}


fn main() {
    env_logger::init();

    let config: Config = toml::from_slice(&std::fs::read("spotr_agent.toml").unwrap()).unwrap();
    info!("Config:");
    info!("{}", config);

    info!("spotr-agent starting");
    let mut agent = Agent {
        sensors: vec!()
    };
    
    agent.start(config).unwrap();

    for sensor in agent.sensors {
        sensor.join().unwrap();
    }

    info!("spotr-agent exiting");
}
