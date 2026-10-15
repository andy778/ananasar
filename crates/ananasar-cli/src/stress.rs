//! The `stress` subcommand: a Modbus/TCP load and robustness generator on top
//! of the `ananasar-stress` library.

use ananasar_net::{parse_endpoint, split_host_port};
use ananasar_stress::{Config, FaultKind, SAFETY_NOTICE, fresh_seed, run};
use clap::Args;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const AFTER_HELP: &str = "\
SAFETY: use this only against equipment you own or are authorised to test.
Stress traffic can slow, confuse or crash a device. The defaults are modest
(4 connections x 10 requests/s for 10 s, reads only). A total rate above
1000 requests/s (or an unlimited rate), more than 64 connections, and any
fault injection are refused unless you add --i-am-authorised.

This is a Modbus/TCP-level tool. It does not replace the link-level
broadcast/multicast/unicast storm tests used for classification; run a raw
packet generator for those. See docs/stress-testing.md.

EXAMPLES:
  ananasar stress 192.168.1.50:502
  ananasar stress plc.local:502 -c 8 -r 50 -d 60 --out results
  ananasar stress 10.0.0.5:502 --rate 5 --ramp-to 400 --ramp-secs 30 -d 40 --i-am-authorised
  ananasar stress 10.0.0.5:502 --mix 3=60,4=20,16=20 --addr-end 199 --qty-max 20
  ananasar stress 10.0.0.5:502 --fault-bad-length 2 --fault-split 5 --i-am-authorised
  ananasar stress 10.0.0.5:502 --churn --churn-interval-ms 50 --half-open 10";

#[derive(Args)]
#[command(after_long_help = AFTER_HELP)]
pub struct StressArgs {
    /// Target as host:port (only for equipment you are authorised to test)
    target: String,
    /// Parallel connections (more than 64 needs --i-am-authorised)
    #[arg(short, long, default_value_t = 4)]
    connections: usize,
    /// Requests per second per connection; 0 = as fast as possible (needs --i-am-authorised)
    #[arg(short, long, default_value_t = 10.0)]
    rate: f64,
    /// Ramp the rate linearly from --rate to this rate (requests/s per connection)
    #[arg(long, value_name = "RATE")]
    ramp_to: Option<f64>,
    /// How long the ramp takes, then the final rate is held [default: the whole run]
    #[arg(long, value_name = "SECS")]
    ramp_secs: Option<f64>,
    /// How long to run, in seconds
    #[arg(short, long, default_value_t = 10.0)]
    duration_secs: f64,
    /// Reply timeout in milliseconds
    #[arg(long, default_value_t = 1000)]
    timeout_ms: u64,
    /// Unit id to address
    #[arg(short, long, default_value_t = 1)]
    uid: u8,
    /// Seed for the request sequence and fault rolls [default: from the clock, printed and recorded]
    #[arg(long)]
    seed: Option<u64>,
    /// Request mix as FC=WEIGHT,... over 3, 4, 6, 16, 22, 23; functions not listed get weight 0.
    /// Writes (6, 16, 22, 23) change data on the target.
    #[arg(long, value_delimiter = ',', value_name = "FC=WEIGHT", default_value = "3=70,4=30", value_parser = parse_weight)]
    mix: Vec<(u8, u32)>,
    /// First register address to use
    #[arg(long, default_value_t = 0)]
    addr_start: u16,
    /// Last register address to use (inclusive)
    #[arg(long, default_value_t = 99)]
    addr_end: u16,
    /// Smallest register quantity per request
    #[arg(long, default_value_t = 1)]
    qty_min: u16,
    /// Largest register quantity per request (limited per function by the protocol)
    #[arg(long, default_value_t = 10)]
    qty_max: u16,
    /// Percent of requests sent with fewer bytes than an MBAP header (needs --i-am-authorised)
    #[arg(long, value_name = "PCT")]
    fault_short_header: Option<f64>,
    /// Percent of requests sent with a non-zero protocol id
    #[arg(long, value_name = "PCT")]
    fault_bad_protocol: Option<f64>,
    /// Percent of requests sent with a wrong length field
    #[arg(long, value_name = "PCT")]
    fault_bad_length: Option<f64>,
    /// Percent of requests sent with the PDU cut short of its declared length
    #[arg(long, value_name = "PCT")]
    fault_truncated_pdu: Option<f64>,
    /// Percent of requests sent with a PDU over 253 bytes
    #[arg(long, value_name = "PCT")]
    fault_oversized_pdu: Option<f64>,
    /// Percent of requests sent together with others in one segment
    #[arg(long, value_name = "PCT")]
    fault_coalesced: Option<f64>,
    /// Percent of requests sent as two segments with a delay between
    #[arg(long, value_name = "PCT")]
    fault_split: Option<f64>,
    /// Requests per segment for --fault-coalesced (2-16)
    #[arg(long, default_value_t = 3)]
    coalesce_count: usize,
    /// Delay between the two segments of --fault-split, in milliseconds
    #[arg(long, default_value_t = 50)]
    split_delay_ms: u64,
    /// How long to watch for a reaction after a malformed frame, in milliseconds
    #[arg(long, default_value_t = 200)]
    fault_wait_ms: u64,
    /// Connection churn: each connection thread connects and disconnects repeatedly
    #[arg(long)]
    churn: bool,
    /// Pause between churn cycles per connection, in milliseconds (0 = none, needs --i-am-authorised)
    #[arg(long, default_value_t = 100)]
    churn_interval_ms: u64,
    /// Send one request on each churn connection before closing it
    #[arg(long, requires = "churn")]
    churn_request: bool,
    /// Hold this many extra idle connections open for the whole run
    #[arg(long, default_value_t = 0, value_name = "N")]
    half_open: usize,
    /// Write the report (JSON, CSV and a per-second timeline CSV) into this directory
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// Confirm you own the target or are authorised to test it. Required for rates above
    /// 1000 requests/s in total, more than 64 connections, and fault injection.
    #[arg(long)]
    i_am_authorised: bool,
}

fn parse_weight(s: &str) -> Result<(u8, u32), String> {
    let (fc, w) = s.split_once('=').ok_or("expected FC=WEIGHT, for example 3=70")?;
    let fc = fc.trim().parse::<u8>().map_err(|_| format!("{fc:?} is not a function code"))?;
    let w = w.trim().parse::<u32>().map_err(|_| format!("{w:?} is not a weight (a whole number)"))?;
    Ok((fc, w))
}

fn secs(name: &str, v: f64) -> Result<Duration, String> {
    Duration::try_from_secs_f64(v).map_err(|_| format!("{name} must be a non-negative number of seconds"))
}

fn build_config(a: &StressArgs) -> Result<Config, String> {
    let (host, port) = split_host_port(&a.target);
    let port = port.ok_or("the target must be host:port, for example 192.168.1.50:502")?;
    let (host, port) = parse_endpoint(host, port)?;
    let mut c = Config::new(&host, port);
    c.unit_id = a.uid;
    c.connections = a.connections;
    c.rate = a.rate;
    c.ramp_to = a.ramp_to;
    c.duration = secs("duration", a.duration_secs)?;
    c.ramp_duration = match a.ramp_secs {
        Some(s) => secs("ramp duration", s)?,
        None => c.duration,
    };
    c.reply_timeout = Duration::from_millis(a.timeout_ms);
    c.seed = a.seed.unwrap_or_else(fresh_seed);
    c.mix.weights = [0; 6];
    for &(fc, w) in &a.mix {
        c.mix.set_weight(fc, w)?;
    }
    c.mix.addr_start = a.addr_start;
    c.mix.addr_end = a.addr_end;
    c.mix.qty_min = a.qty_min;
    c.mix.qty_max = a.qty_max;
    for (kind, pct) in [
        (FaultKind::ShortHeader, a.fault_short_header),
        (FaultKind::BadProtocol, a.fault_bad_protocol),
        (FaultKind::BadLength, a.fault_bad_length),
        (FaultKind::TruncatedPdu, a.fault_truncated_pdu),
        (FaultKind::OversizedPdu, a.fault_oversized_pdu),
        (FaultKind::Coalesced, a.fault_coalesced),
        (FaultKind::Split, a.fault_split),
    ] {
        c.faults.set(kind, pct.unwrap_or(0.0));
    }
    c.coalesce_count = a.coalesce_count;
    c.split_delay = Duration::from_millis(a.split_delay_ms);
    c.fault_wait = Duration::from_millis(a.fault_wait_ms);
    c.churn = a.churn;
    c.churn_interval = Duration::from_millis(a.churn_interval_ms);
    c.churn_request = a.churn_request;
    c.half_open = a.half_open;
    c.authorised = a.i_am_authorised;
    Ok(c)
}

pub fn run_stress(args: &StressArgs) -> i32 {
    println!("NOTICE: {SAFETY_NOTICE}");
    let cfg = match build_config(args).and_then(|c| c.check().map(|()| c)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    println!(
        "stress {}:{}: {} {}, {}, {:.0} s, seed {} (repeat with --seed {})",
        cfg.host,
        cfg.port,
        cfg.connections,
        if cfg.churn { "churn threads" } else { "connections" },
        rate_text(&cfg),
        cfg.duration.as_secs_f64(),
        cfg.seed,
        cfg.seed
    );
    let report = match run(&cfg, &AtomicBool::new(false), &mut |tick| println!("{}", tick.line())) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    println!();
    print!("{}", report.text());
    if let Some(dir) = &args.out {
        match report.write_to(dir) {
            Ok(files) => {
                for f in files {
                    println!("wrote {}", f.display());
                }
            }
            Err(e) => {
                eprintln!("cannot write the report to {}: {e}", dir.display());
                return 1;
            }
        }
    }
    0
}

fn rate_text(c: &Config) -> String {
    if c.churn {
        return format!("churn every {} ms", c.churn_interval.as_millis());
    }
    match c.ramp_to {
        Some(end) => format!("{}-{} req/s each (ramp over {:.0} s)", c.rate, end, c.ramp_duration.as_secs_f64()),
        None if c.rate == 0.0 => "unlimited rate".to_owned(),
        None => format!("{} req/s each", c.rate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Wrap {
        #[command(subcommand)]
        cmd: Sub,
    }
    #[derive(clap::Subcommand)]
    enum Sub {
        Stress(StressArgs),
    }

    fn parse(line: &str) -> Result<StressArgs, clap::Error> {
        let mut argv = vec!["ananasar"];
        argv.extend(line.split_whitespace());
        Wrap::try_parse_from(argv).map(|w| match w.cmd {
            Sub::Stress(a) => a,
        })
    }

    #[test]
    fn defaults_are_modest_and_need_no_authorisation() {
        let c = build_config(&parse("stress 127.0.0.1:502").unwrap()).unwrap();
        assert_eq!((c.connections, c.rate, c.duration), (4, 10.0, Duration::from_secs(10)));
        assert_eq!(c.mix.weights, [70, 30, 0, 0, 0, 0]);
        assert_eq!(c.check(), Ok(()));
    }

    #[test]
    fn target_and_ports_are_validated() {
        for bad in ["stress plc.local", "stress plc.local:0", "stress plc.local:70000", "stress :502", "stress host:x"] {
            assert!(build_config(&parse(bad).unwrap()).is_err(), "{bad}");
        }
        assert!(build_config(&parse("stress [::1]:1502").unwrap()).is_ok());
    }

    #[test]
    fn percentages_rates_and_mix_are_validated() {
        let bad = |line: &str| build_config(&parse(line).unwrap()).and_then(|c| c.check().map(|()| c)).unwrap_err();
        assert!(bad("stress h:502 --fault-split 120 --i-am-authorised").contains("percentage"));
        assert!(bad("stress h:502 --fault-split=-1 --i-am-authorised").contains("percentage"));
        assert!(bad("stress h:502 --rate=-5").contains("rate"));
        assert!(bad("stress h:502 --mix 9=1").contains("function code"));
        assert!(bad("stress h:502 --mix 3=0").contains("mix"));
        assert!(bad("stress h:502 --duration-secs 0").contains("duration"));
        assert!(parse("stress h:502 --mix three=1").is_err());
        assert!(parse("stress h:502 --churn-request").is_err());
    }

    #[test]
    fn risky_options_are_refused_without_the_flag() {
        for line in [
            "stress h:502 --fault-bad-length 1",
            "stress h:502 --rate 0",
            "stress h:502 --rate 300",
            "stress h:502 --connections 65",
            "stress h:502 --rate 1 --ramp-to 2000",
            "stress h:502 --churn --churn-interval-ms 0",
        ] {
            let c = build_config(&parse(line).unwrap()).unwrap();
            assert!(c.check().unwrap_err().contains("--i-am-authorised"), "{line}");
            let ok = build_config(&parse(&format!("{line} --i-am-authorised")).unwrap()).unwrap();
            assert_eq!(ok.check(), Ok(()), "{line}");
        }
    }

    #[test]
    fn reproduce_args_parse_back_to_the_same_run() {
        let line = "stress 10.0.0.5:502 -c 3 --rate 5 --ramp-to 40 --ramp-secs 5 -d 8 --timeout-ms 500 --uid 3 --seed 77 \
                    --mix 3=1,16=2,23=1 --addr-start 10 --addr-end 50 --qty-min 2 --qty-max 6 \
                    --fault-split 4 --fault-bad-length 1.5 --split-delay-ms 20 --coalesce-count 4 --fault-wait-ms 90 \
                    --half-open 2 --i-am-authorised";
        let cfg = build_config(&parse(line).unwrap()).unwrap();
        let again = build_config(&parse(&cfg.reproduce_args()).unwrap()).unwrap();
        assert_eq!(cfg, again);
        let churn = build_config(&parse("stress h:1 --churn --churn-interval-ms 40 --churn-request --seed 5").unwrap()).unwrap();
        assert_eq!(build_config(&parse(&churn.reproduce_args()).unwrap()).unwrap(), churn);
    }
}
