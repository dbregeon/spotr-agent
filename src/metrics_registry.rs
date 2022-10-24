use std::collections::HashMap;

use prometheus::{
    core::{AtomicU64, GenericGauge},
    Error, Registry,
};
use spotr_sensing::{ProcessStats, SensorOutput, Stat};

type Result<T> = std::result::Result<T, ErrorCode>;
type ProcessKey = (u32, String);

pub struct MetricsRegistry {
    registry: Registry,
    process_collectors: HashMap<ProcessKey, ProcessStatsCollector>,
    mount_collectors: HashMap<String, MountStatsCollector>,
}

#[derive(Debug)]
pub enum ErrorCode {
    Error(String),
}

impl From<Error> for ErrorCode {
    fn from(e: Error) -> Self {
        match e {
            Error::Msg(m) => ErrorCode::Error(m),
            ex => ErrorCode::Error(format!("Unexpected error: {:?}", ex)),
        }
    }
}

impl MetricsRegistry {
    pub fn new(registry: Registry) -> Self {
        MetricsRegistry {
            registry,
            process_collectors: HashMap::new(),
            mount_collectors: HashMap::new(),
        }
    }

    pub fn record(&mut self, sensor_output: SensorOutput) -> Result<()> {
        match sensor_output {
            SensorOutput::Process { pid, stat } => self.record_process_metrics(pid, stat),
            SensorOutput::MountPoint { name, size, free } => {
                self.record_mount_metrics(name, size, free)
            }
        }
    }

    fn record_process_metrics(&mut self, pid: u32, stat: ProcessStats) -> Result<()> {
        if let ProcessStats::Stat(Stat {
            utime, stime, comm, ..
        }) = stat
        {
            let key = (pid, comm.clone());
            if !self.process_collectors.contains_key(&key) {
                let result = ProcessStatsCollector::new(pid, comm)?;
                self.registry.register(Box::new(result.cpu_time.clone()))?;
                self.process_collectors.insert(key.clone(), result);
            }
            let collector = &self.process_collectors[&key];
            Ok(collector.cpu_time.set(utime + stime))
        } else {
            Ok(())
        }
    }

    fn record_mount_metrics(&mut self, name: String, size: u64, free: u64) -> Result<()> {
        let key = name.clone();
        if !self.mount_collectors.contains_key(&key) {
            let result = MountStatsCollector::new(name)?;
            self.registry.register(Box::new(result.size.clone()))?;
            self.registry.register(Box::new(result.free.clone()))?;
            self.mount_collectors.insert(key.clone(), result);
        }
        let collector = &self.mount_collectors[&key];
        collector.size.set(size);
        collector.free.set(free);
        Ok(())
    }
}

struct ProcessStatsCollector {
    cpu_time: GenericGauge<AtomicU64>,
}

impl ProcessStatsCollector {
    fn new(pid: u32, comm: String) -> Result<Self> {
        let cpu_time = GenericGauge::with_opts(prometheus::Opts {
            namespace: "spotr".to_string(),
            subsystem: "processes".to_string(),
            name: "cpu_ticks".to_string(),
            help: "CPU ticks used by the process".to_string(),
            const_labels: [
                ("pid".to_string(), format!("{}", pid)),
                ("command".to_string(), comm),
            ]
            .into(),
            variable_labels: vec![],
        })
        .map_err(|e| ErrorCode::from(e))?;
        Ok(ProcessStatsCollector { cpu_time })
    }
}

struct MountStatsCollector {
    size: GenericGauge<AtomicU64>,
    free: GenericGauge<AtomicU64>,
}

impl MountStatsCollector {
    fn new(name: String) -> Result<Self> {
        let size = GenericGauge::with_opts(prometheus::Opts {
            namespace: "spotr".to_string(),
            subsystem: "mounts".to_string(),
            name: "size".to_string(),
            help: "Size of the mount".to_string(),
            const_labels: [("name".to_string(), name.clone())].into(),
            variable_labels: vec![],
        })
        .map_err(|e| ErrorCode::from(e))?;
        let free = GenericGauge::with_opts(prometheus::Opts {
            namespace: "spotr".to_string(),
            subsystem: "mounts".to_string(),
            name: "free".to_string(),
            help: "Free size of the mount".to_string(),
            const_labels: [("name".to_string(), name)].into(),
            variable_labels: vec![],
        })
        .map_err(|e| ErrorCode::from(e))?;
        Ok(MountStatsCollector { size, free })
    }
}
