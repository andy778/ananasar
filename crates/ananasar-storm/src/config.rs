//! Run parameters, their validation and the safety checks.

use crate::fault::FaultKind;
use crate::rng::Rng;
use std::time::Duration;

/// Total request rate (all connections) above which authorisation is required.
pub const MODEST_TOTAL_RATE: f64 = 1000.0;
/// Connection count above which authorisation is required.
pub const MODEST_CONNECTIONS: usize = 64;
/// Hard cap on connections, authorised or not.
pub const MAX_CONNECTIONS: usize = 1024;
/// Hard cap on the per-connection rate.
pub const MAX_RATE: f64 = 1_000_000.0;
/// Function codes of the request mix, in weight order.
pub const MIX_FUNCTIONS: [u8; 6] = [3, 4, 6, 16, 22, 23];

/// The request mix: function weights and the address and quantity ranges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mix {
    /// Weights for FC3, FC4, FC6, FC16, FC22 and FC23, in that order.
    pub weights: [u32; 6],
    /// First address used (inclusive).
    pub addr_start: u16,
    /// Last address used (inclusive).
    pub addr_end: u16,
    /// Smallest quantity of registers.
    pub qty_min: u16,
    /// Largest quantity of registers (further limited per function).
    pub qty_max: u16,
}

impl Default for Mix {
    /// Reads only: writes change the device's data, so they are opt-in.
    fn default() -> Self {
        Self { weights: [70, 30, 0, 0, 0, 0], addr_start: 0, addr_end: 99, qty_min: 1, qty_max: 10 }
    }
}

impl Mix {
    /// Set the weight of function code `fc`.
    pub fn set_weight(&mut self, fc: u8, weight: u32) -> Result<(), String> {
        let slot = MIX_FUNCTIONS.iter().position(|&f| f == fc).and_then(|i| self.weights.get_mut(i));
        match slot {
            Some(w) => {
                *w = weight;
                Ok(())
            }
            None => Err(format!("function code {fc} is not in the mix (use 3, 4, 6, 16, 22 or 23)")),
        }
    }

    fn total(&self) -> u64 {
        self.weights.iter().map(|&w| u64::from(w)).sum()
    }

    /// Whether any write function has a non-zero weight.
    pub fn writes(&self) -> bool {
        self.weights.iter().skip(2).any(|&w| w > 0)
    }

    /// Choose a function code by weight. An all-zero mix yields FC3.
    pub fn pick(&self, rng: &mut Rng) -> u8 {
        let mut roll = rng.below(self.total());
        for (&fc, &w) in MIX_FUNCTIONS.iter().zip(&self.weights) {
            let w = u64::from(w);
            if roll < w {
                return fc;
            }
            roll = roll.saturating_sub(w);
        }
        3
    }
}

/// Fault injection: the percentage of requests that get each fault. The
/// percentages are exclusive per request and must add up to at most 100.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Faults {
    /// Percent per kind, in [`FaultKind::ALL`] order.
    pub percent: [f64; 7],
}

impl Faults {
    /// The percentage for `kind`.
    pub fn get(&self, kind: FaultKind) -> f64 {
        FaultKind::ALL.iter().position(|&k| k == kind).and_then(|i| self.percent.get(i)).copied().unwrap_or(0.0)
    }

    /// Set the percentage for `kind`.
    pub fn set(&mut self, kind: FaultKind, percent: f64) {
        if let Some(p) = FaultKind::ALL.iter().position(|&k| k == kind).and_then(|i| self.percent.get_mut(i)) {
            *p = percent;
        }
    }

    /// Whether any fault is enabled.
    pub fn any(&self) -> bool {
        self.percent.iter().any(|&p| p > 0.0)
    }

    fn sum(&self) -> f64 {
        self.percent.iter().sum()
    }

    /// Choose the fault for the next request, if any.
    pub fn pick(&self, rng: &mut Rng) -> Option<FaultKind> {
        let mut roll = rng.percent();
        for (&kind, &p) in FaultKind::ALL.iter().zip(&self.percent) {
            if roll < p {
                return Some(kind);
            }
            roll -= p;
        }
        None
    }
}

/// Everything that defines a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Target host name or address.
    pub host: String,
    /// Target TCP port.
    pub port: u16,
    /// Unit id in requests.
    pub unit_id: u8,
    /// Parallel connections.
    pub connections: usize,
    /// Requests per second per connection; 0 means as fast as possible.
    pub rate: f64,
    /// Ramp linearly to this rate (per connection) instead of holding `rate`.
    pub ramp_to: Option<f64>,
    /// How long the ramp takes; the run keeps the final rate afterwards.
    pub ramp_duration: Duration,
    /// Total run time.
    pub duration: Duration,
    /// How long to wait for a reply.
    pub reply_timeout: Duration,
    /// Request mix.
    pub mix: Mix,
    /// Seed for the request sequence and the fault rolls.
    pub seed: u64,
    /// Fault injection percentages.
    pub faults: Faults,
    /// Delay between the two segments of a split request.
    pub split_delay: Duration,
    /// Requests per segment for the coalesced fault.
    pub coalesce_count: usize,
    /// How long to wait for any reaction after a malformed frame.
    pub fault_wait: Duration,
    /// Connection churn instead of a request stream.
    pub churn: bool,
    /// Pause between churn cycles per connection; zero means none.
    pub churn_interval: Duration,
    /// Send one request on each churn connection before closing it.
    pub churn_request: bool,
    /// Extra connections held open and idle for the whole run.
    pub half_open: usize,
    /// The operator confirmed they may test this target.
    pub authorised: bool,
}

impl Config {
    /// A modest default run against `host:port`: 4 connections at 10
    /// requests per second each for 10 seconds, reads only.
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_owned(),
            port,
            unit_id: 1,
            connections: 4,
            rate: 10.0,
            ramp_to: None,
            ramp_duration: Duration::from_secs(10),
            duration: Duration::from_secs(10),
            reply_timeout: Duration::from_secs(1),
            mix: Mix::default(),
            seed: 0,
            faults: Faults::default(),
            split_delay: Duration::from_millis(50),
            coalesce_count: 3,
            fault_wait: Duration::from_millis(200),
            churn: false,
            churn_interval: Duration::from_millis(100),
            churn_request: false,
            half_open: 0,
            authorised: false,
        }
    }

    /// The request rate (per connection) `elapsed` into the run. 0 means
    /// unlimited.
    pub fn rate_at(&self, elapsed: Duration) -> f64 {
        let Some(end) = self.ramp_to else { return self.rate };
        let span = self.ramp_duration.as_secs_f64();
        let t = if span > 0.0 { (elapsed.as_secs_f64() / span).clamp(0.0, 1.0) } else { 1.0 };
        self.rate + (end - self.rate) * t
    }

    /// The highest total request (or churn connect) rate over all connections,
    /// or `None` when it is unlimited.
    pub fn peak_total_rate(&self) -> Option<f64> {
        let conns = self.connections as f64;
        if self.churn {
            let secs = self.churn_interval.as_secs_f64();
            return (secs > 0.0).then(|| conns / secs);
        }
        let peak = self.rate.max(self.ramp_to.unwrap_or(0.0));
        (peak > 0.0).then_some(peak * conns)
    }

    /// The reasons this run needs the operator's authorisation flag.
    pub fn authorisation_reasons(&self) -> Vec<String> {
        let mut why = Vec::new();
        match self.peak_total_rate() {
            None => why.push("an unlimited request rate".to_owned()),
            Some(r) if r > MODEST_TOTAL_RATE => {
                why.push(format!("a total rate of {r:.0}/s (more than {MODEST_TOTAL_RATE:.0}/s)"));
            }
            Some(_) => {}
        }
        let conns = self.connections.saturating_add(self.half_open);
        if conns > MODEST_CONNECTIONS {
            why.push(format!("{conns} connections (more than {MODEST_CONNECTIONS})"));
        }
        if self.faults.any() {
            why.push("fault injection".to_owned());
        }
        why
    }

    /// Check the arguments and the safety rules.
    pub fn check(&self) -> Result<(), String> {
        self.validate()?;
        let why = self.authorisation_reasons();
        if !why.is_empty() && !self.authorised {
            return Err(format!(
                "refusing to run with {}. This can disturb or disable the target. \
                 If you own the equipment or are authorised to test it, add --i-am-authorised.",
                why.join(", ")
            ));
        }
        Ok(())
    }

    /// Check the arguments (not the authorisation).
    pub fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty() || self.host.contains(char::is_whitespace) {
            return Err("host is empty or contains spaces".into());
        }
        if self.port == 0 {
            return Err("port must be 1-65535".into());
        }
        if !(1..=MAX_CONNECTIONS).contains(&self.connections) {
            return Err(format!("connections must be 1-{MAX_CONNECTIONS}"));
        }
        if self.half_open > MAX_CONNECTIONS {
            return Err(format!("half-open connections must be at most {MAX_CONNECTIONS}"));
        }
        let rate_ok = |r: f64| r.is_finite() && (0.0..=MAX_RATE).contains(&r);
        if !rate_ok(self.rate) {
            return Err(format!("rate must be 0-{MAX_RATE:.0} requests per second (0 = unlimited)"));
        }
        if let Some(end) = self.ramp_to {
            if !rate_ok(end) || end <= 0.0 || self.rate <= 0.0 {
                return Err(format!("a ramp needs a start and end rate of 0 < rate <= {MAX_RATE:.0}"));
            }
            if self.ramp_duration.is_zero() {
                return Err("ramp duration must be at least 1 ms".into());
            }
        }
        if self.duration < Duration::from_secs(1) || self.duration > Duration::from_secs(86_400) {
            return Err("duration must be between 1 second and 24 hours".into());
        }
        if self.reply_timeout < Duration::from_millis(10) || self.reply_timeout > Duration::from_secs(60) {
            return Err("reply timeout must be between 10 ms and 60 s".into());
        }
        if self.mix.total() == 0 {
            return Err("the request mix has no function with a weight above 0".into());
        }
        if self.mix.addr_start > self.mix.addr_end {
            return Err("address start is above address end".into());
        }
        if self.mix.qty_min == 0 || self.mix.qty_min > self.mix.qty_max || self.mix.qty_max > 125 {
            return Err("quantities must satisfy 1 <= min <= max <= 125".into());
        }
        for kind in FaultKind::ALL {
            let p = self.faults.get(kind);
            if !p.is_finite() || !(0.0..=100.0).contains(&p) {
                return Err(format!("--fault-{} must be a percentage from 0 to 100", kind.name()));
            }
        }
        if self.faults.sum() > 100.0 {
            return Err("the fault percentages add up to more than 100".into());
        }
        if !(2..=16).contains(&self.coalesce_count) {
            return Err("coalesce count must be 2-16".into());
        }
        if self.split_delay > Duration::from_secs(10) || self.fault_wait > Duration::from_secs(10) {
            return Err("split delay and fault wait must be at most 10 s".into());
        }
        Ok(())
    }

    /// The `ananasar storm` arguments that repeat this run exactly (the
    /// operator's authorisation flag is included only if it was given).
    pub fn reproduce_args(&self) -> String {
        let mut a = vec![
            "storm".to_owned(),
            format!("{}:{}", self.host, self.port),
            format!("--uid {}", self.unit_id),
            format!("--connections {}", self.connections),
            format!("--rate {}", self.rate),
            format!("--duration-secs {}", self.duration.as_secs_f64()),
            format!("--timeout-ms {}", self.reply_timeout.as_millis()),
            format!("--seed {}", self.seed),
        ];
        if let Some(end) = self.ramp_to {
            a.push(format!("--ramp-to {end}"));
            a.push(format!("--ramp-secs {}", self.ramp_duration.as_secs_f64()));
        }
        let mix: Vec<String> = MIX_FUNCTIONS.iter().zip(&self.mix.weights).map(|(fc, w)| format!("{fc}={w}")).collect();
        a.push(format!("--mix {}", mix.join(",")));
        a.push(format!("--addr-start {} --addr-end {}", self.mix.addr_start, self.mix.addr_end));
        a.push(format!("--qty-min {} --qty-max {}", self.mix.qty_min, self.mix.qty_max));
        for kind in FaultKind::ALL {
            let p = self.faults.get(kind);
            if p > 0.0 {
                a.push(format!("--fault-{} {p}", kind.name()));
            }
        }
        a.push(format!("--split-delay-ms {}", self.split_delay.as_millis()));
        a.push(format!("--coalesce-count {}", self.coalesce_count));
        a.push(format!("--fault-wait-ms {}", self.fault_wait.as_millis()));
        if self.churn {
            a.push("--churn".to_owned());
            a.push(format!("--churn-interval-ms {}", self.churn_interval.as_millis()));
            if self.churn_request {
                a.push("--churn-request".to_owned());
            }
        }
        if self.half_open > 0 {
            a.push(format!("--half-open {}", self.half_open));
        }
        if self.authorised {
            a.push("--i-am-authorised".to_owned());
        }
        a.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::new("127.0.0.1", 502)
    }

    #[test]
    fn fixed_rate_is_constant() {
        let c = cfg();
        assert!((c.rate_at(Duration::ZERO) - 10.0).abs() < 1e-9);
        assert!((c.rate_at(Duration::from_secs(99)) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn ramp_is_linear_and_then_holds() {
        let c = Config { rate: 10.0, ramp_to: Some(110.0), ramp_duration: Duration::from_secs(10), ..cfg() };
        let at = |ms| c.rate_at(Duration::from_millis(ms));
        assert!((at(0) - 10.0).abs() < 1e-9);
        assert!((at(5_000) - 60.0).abs() < 1e-9);
        assert!((at(10_000) - 110.0).abs() < 1e-9);
        assert!((at(60_000) - 110.0).abs() < 1e-9);
        // Ramping down works the same way.
        let d = Config { rate: 100.0, ramp_to: Some(20.0), ramp_duration: Duration::from_secs(4), ..cfg() };
        assert!((d.rate_at(Duration::from_secs(1)) - 80.0).abs() < 1e-9);
    }

    #[test]
    fn defaults_are_modest_and_pass_without_authorisation() {
        let c = cfg();
        assert_eq!((c.connections, c.duration, c.reply_timeout), (4, Duration::from_secs(10), Duration::from_secs(1)));
        assert!(c.authorisation_reasons().is_empty());
        assert_eq!(c.check(), Ok(()));
        assert!(!c.mix.writes());
    }

    #[test]
    fn high_rate_many_connections_and_faults_need_the_flag() {
        let fast = Config { rate: 300.0, ..cfg() }; // 4 x 300 = 1200/s
        assert!(fast.check().unwrap_err().contains("--i-am-authorised"));
        let unlimited = Config { rate: 0.0, ..cfg() };
        assert!(unlimited.check().unwrap_err().contains("unlimited"));
        let many = Config { connections: 65, ..cfg() };
        assert!(many.check().unwrap_err().contains("65 connections"));
        let held = Config { connections: 60, half_open: 5, ..cfg() };
        assert!(held.check().is_err());
        let mut faulty = cfg();
        faulty.faults.set(FaultKind::BadLength, 1.0);
        assert!(faulty.check().unwrap_err().contains("fault injection"));
        let churn = Config { churn: true, churn_interval: Duration::ZERO, ..cfg() };
        assert!(churn.check().is_err());
        // The same runs pass once authorised.
        for c in [fast, unlimited, many, faulty, churn] {
            assert_eq!(Config { authorised: true, ..c }.check(), Ok(()));
        }
        // Exactly at the limits stays allowed.
        assert_eq!(Config { connections: 64, rate: 15.625, ..cfg() }.check(), Ok(()));
    }

    #[test]
    fn a_ramp_counts_its_peak() {
        let c = Config { rate: 1.0, ramp_to: Some(500.0), ..cfg() };
        assert_eq!(c.peak_total_rate(), Some(2000.0));
        assert!(c.check().is_err());
    }

    #[test]
    fn rejects_bad_arguments() {
        let bad = |c: Config| c.validate().unwrap_err();
        assert!(bad(Config::new("", 502)).contains("host"));
        assert!(bad(Config::new("a b", 502)).contains("host"));
        assert!(bad(Config::new("h", 0)).contains("port"));
        assert!(bad(Config { connections: 0, ..cfg() }).contains("connections"));
        assert!(bad(Config { connections: 5000, ..cfg() }).contains("connections"));
        assert!(bad(Config { rate: -1.0, ..cfg() }).contains("rate"));
        assert!(bad(Config { rate: f64::NAN, ..cfg() }).contains("rate"));
        assert!(bad(Config { rate: 5.0, ramp_to: Some(0.0), ..cfg() }).contains("ramp"));
        assert!(bad(Config { rate: 0.0, ramp_to: Some(5.0), ..cfg() }).contains("ramp"));
        assert!(bad(Config { duration: Duration::ZERO, ..cfg() }).contains("duration"));
        assert!(bad(Config { reply_timeout: Duration::ZERO, ..cfg() }).contains("timeout"));
        assert!(bad(Config { mix: Mix { weights: [0; 6], ..Mix::default() }, ..cfg() }).contains("mix"));
        assert!(bad(Config { mix: Mix { addr_start: 9, addr_end: 3, ..Mix::default() }, ..cfg() }).contains("address"));
        assert!(bad(Config { mix: Mix { qty_min: 0, ..Mix::default() }, ..cfg() }).contains("quantities"));
        assert!(bad(Config { mix: Mix { qty_max: 126, ..Mix::default() }, ..cfg() }).contains("quantities"));
        let mut c = cfg();
        c.faults.set(FaultKind::Split, 101.0);
        assert!(bad(c).contains("percentage"));
        let mut c = cfg();
        c.faults.set(FaultKind::Split, 60.0);
        c.faults.set(FaultKind::BadLength, 60.0);
        assert!(bad(c).contains("100"));
        assert!(bad(Config { coalesce_count: 1, ..cfg() }).contains("coalesce"));
    }

    #[test]
    fn mix_weights_select_functions() {
        let mut m = Mix::default();
        assert!(m.set_weight(5, 1).is_err());
        assert!(m.set_weight(16, 5).is_ok());
        assert!(m.writes());
        let only16 = Mix { weights: [0, 0, 0, 5, 0, 0], ..Mix::default() };
        let mut rng = Rng::derive(1, 0);
        assert!((0..100).all(|_| only16.pick(&mut rng) == 16));
    }

    #[test]
    fn fault_picks_follow_percentages() {
        let mut f = Faults::default();
        f.set(FaultKind::BadProtocol, 25.0);
        let mut rng = Rng::derive(3, 0);
        let hits = (0..20_000).filter(|_| f.pick(&mut rng) == Some(FaultKind::BadProtocol)).count();
        assert!((4_500..5_500).contains(&hits), "{hits}");
        assert_eq!(Faults::default().pick(&mut rng), None);
    }

    #[test]
    fn reproduce_args_names_the_seed_and_parameters() {
        let mut c = Config { seed: 77, ramp_to: Some(40.0), churn: true, ..cfg() };
        c.faults.set(FaultKind::Split, 5.0);
        let s = c.reproduce_args();
        for want in ["--seed 77", "--ramp-to 40", "--fault-split 5", "--churn", "127.0.0.1:502", "--mix 3=70,4=30,6=0"] {
            assert!(s.contains(want), "{s} lacks {want}");
        }
    }
}
