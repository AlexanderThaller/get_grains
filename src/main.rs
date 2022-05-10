use clap::{
    crate_version,
    load_yaml,
    value_t,
    App,
    ArgMatches as Args,
};
use error_chain::ChainedError;
use log::{
    debug,
    error,
    trace,
    warn,
    Level,
};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::BTreeMap as DataMap,
    fs,
    fs::File,
    io::{
        self,
        Read,
        Write,
    },
    path::PathBuf,
    process::Command,
    str::from_utf8,
};

mod host;

mod errors {
    use error_chain::error_chain;

    // Create the Error, ErrorKind, ResultExt, and Result types
    error_chain! {
        errors {
          DoNotKnowCommand(c: String) {
            description("do not know the given command")
            display("do not know the given command: '{}'", c)
          }
          NoCommand {
            description("no command given")
            display("no command given")
          }
        }
    }
}

use crate::errors::{
    Error,
    ResultExt,
};

fn main() {
    if let Err(e) = run() {
        error!("error while running: {}", e);
        for e in e.iter().skip(1) {
            error!("caused by: {}", e);
        }

        // The backtrace is not always generated. Try to run this example
        // with `RUST_BACKTRACE=1`.
        if let Some(backtrace) = e.backtrace() {
            error!("backtrace: {:?}", backtrace);
        }

        ::std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    let yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(yaml).version(crate_version!()).get_matches();

    {
        let loglevel: Level = value_t!(matches, "loglevel", Level)
            .chain_err(|| "can not get the loglevel from args")?;
        loggerv::init_with_level(loglevel)
            .chain_err(|| "can not set the loglevel of the environment logger")?;
    }
    trace!("matches: {:#?}", matches);

    match matches.clone().subcommand_name() {
        Some("run_salt") => run_run_salt(&matches.subcommand().unwrap().1)
            .chain_err(|| "problem while running run_salt"),

        Some("read_file") => run_read_file(&matches.subcommand().unwrap().1)
            .chain_err(|| "problem while running read_file"),

        Some(command) => Err(errors::ErrorKind::DoNotKnowCommand(command.to_owned()).into()),
        None => Err(errors::ErrorKind::NoCommand.into()),
    }
}

fn run_run_salt(args: &Args) -> Result<(), Error> {
    trace!("run_run_salt args: {:#?}", args);

    let salt_target = args.value_of("salt_target").ok_or("no salt_target given")?;
    debug!("salt_target: {}", salt_target);

    let grainsdir: PathBuf = PathBuf::from(args.value_of("grainsdir").ok_or("no graindir given")?);
    debug!("grainsdir: {:?}", grainsdir);

    let timeout = value_t!(args, "timeout", usize).chain_err(|| "can not get timeout from args")?;
    debug!("timeout: {}", timeout);

    let batchsize =
        value_t!(args, "batchsize", usize).chain_err(|| "can not get batchsize from args")?;
    debug!("batchsize: {}", batchsize);

    let compound_target = args.is_present("compound_target");
    debug!("compound_target: {}", compound_target);

    let save_folder = if args.is_present("save_folder") {
        Some(PathBuf::from(
            args.value_of("save_folder").ok_or("no save_folder given")?,
        ))
    } else {
        None
    };
    debug!("save_folder: {:?}", save_folder);

    let minions_data = get_minions_data_from_salt(salt_target, compound_target, timeout, batchsize)
        .chain_err(|| "can not get minions data from salt")?;

    if let Some(mut path) = save_folder.clone() {
        fs::create_dir_all(&path).chain_err(|| "can not create folder for saving salt output")?;

        path.push("output.json");

        debug!("Salt output path: {:?}", path);

        let mut file = File::create(&path)
            .chain_err(|| "can not create file for writing minions_data to save output")?;

        file.write(minions_data.as_bytes())
            .chain_err(|| "can not write minions_data to path file")?;

        debug!("wrote salt output to {:?}", path.to_str());
    }

    let mut minions = parse_minions_from_minions_data(&minions_data)
        .chain_err(|| "can not parse minions from minions data")?;

    for (hostid, host) in minions.clone() {
        if host.is_success() {
            continue;
        }

        let max_retries = 3;
        for retry_count in { 0..max_retries } {
            debug!(
                "trying again to get grains for {} (retry {})",
                hostid, retry_count
            );
            let minion_data = match get_minions_data_from_salt(
                hostid.as_str(),
                compound_target,
                timeout / max_retries,
                batchsize,
            ) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let minion = parse_minions_from_minions_data(&minion_data)
                .chain_err(|| "can not parse minion from minion data")?;

            let new_host = minion.values().next().unwrap();

            if let Some(path) = save_folder.clone() {
                let mut path = path.join("retry").join(hostid.clone());

                fs::create_dir_all(path.clone())
                    .chain_err(|| "can not create folder for saving retry data for minion")?;

                path.push(format!("{}.json", retry_count.to_string()));

                debug!("Minion retry output path: {:?}", path);

                let mut file = File::create(&path).chain_err(|| {
                    "can not create file for writing retry minions_data to save output"
                })?;

                file.write(minions_data.as_bytes())
                    .chain_err(|| "can not write minions_data to path file")?;

                debug!("wrote salt output to {:?}", path.to_str());
            }

            if host.is_success() {
                minions.insert(hostid.clone(), new_host.clone());
                break;
            }
        }
    }

    serialize_minions(minions, &grainsdir)
        .chain_err(|| "can not serialize minions to json files")?;

    Ok(())
}

fn run_read_file(args: &Args) -> Result<(), Error> {
    trace!("run_read_file args: {:#?}", args);

    let grainsdir: PathBuf = PathBuf::from(args.value_of("grainsdir").ok_or("no graindir given")?);
    trace!("grainsdir: {:?}", grainsdir);

    let minions_data = {
        let input = args.value_of("input").ok_or("no input given")?;

        match input {
            "-" => {
                let mut buffer = String::new();
                io::stdin()
                    .read_to_string(&mut buffer)
                    .expect("can not read from stdin");
                buffer
            }
            _ => {
                let mut file = File::open(input).expect("can not open input file");
                let mut input = String::new();
                file.read_to_string(&mut input)
                    .expect("can not read input file to string");
                input
            }
        }
    };

    let minions = parse_minions_from_minions_data(&minions_data)
        .chain_err(|| "can not parse minions from minions data")?;

    serialize_minions(minions, &grainsdir)
        .chain_err(|| "can not serialize minions to json files")?;

    Ok(())
}

fn get_minions_data_from_salt(
    minions: &str,
    compound_target: bool,
    timeout: usize,
    batchsize: usize,
) -> std::result::Result<String, errors::Error> {
    let command_string = format!(
        "salt {compound_target} '{minions}' -t {timeout} -b {batchsize} --out json --static \
         --state-verbose=false grains.items",
        minions = minions,
        timeout = timeout,
        batchsize = batchsize,
        compound_target = if compound_target { "-C" } else { "" }
    );

    debug!("runing salt with command: {}", command_string);

    let mut command = Command::new("sh");
    command.args(&["-c", command_string.as_str()]);

    trace!("command: {:#?}", command);

    let output = command
        .output()
        .chain_err(|| "problem while running salt")?;

    debug!("finished running salt");

    if output.status.success() {
        let stdout = from_utf8(output.stdout.as_slice())
            .chain_err(|| "can not convert stdout to utf8 str")?
            .to_owned();
        Ok(stdout)
    } else {
        let stderr = from_utf8(output.stderr.as_slice())
            .chain_err(|| "can not convert stderr to utf8 str")?;
        Err(format!(
            "exit code of salt command is not zero: {}:\n{}",
            output.status, stderr
        )
        .into())
    }
}

fn parse_minions_from_minions_data(
    minions_data: &str,
) -> std::result::Result<DataMap<String, host::Host>, errors::Error> {
    let mut minions = DataMap::default();

    let minions_data = {
        let minions_data = {
            // match all hosts that have not returned as they are not in the
            // json data
            // format is normally like "Minion pricesearch did not respond. No
            // job will be
            // sent."
            let regex = Regex::new(r"(?m)^Minion (\S*) did not respond\. No job will be sent\.$")
                .chain_err(|| "regex for catching not returned minions is not valid")?;

            let mut failed = Vec::new();
            for host in regex.captures_iter(minions_data) {
                failed.push(host[1].to_string());
            }

            let data = regex.replace_all(minions_data, "").into_owned();

            trace!("no_return: {:#?}", failed);

            for minion in failed {
                minions.insert(
                    minion.clone(),
                    host::Host {
                        hostname: minion,
                        status: host::HostStatus::DidNotRespond,
                        ..host::Host::default()
                    },
                );
            }

            data
        };

        {
            let regex = Regex::new(
                r"(?m)^minion (\S*) was already deleted from tracker, probably a duplicate key$",
            )
            .chain_err(|| "regex for catching deleted minions is not valid")?;

            let mut failed = Vec::new();
            for host in regex.captures_iter(minions_data.as_str()) {
                failed.push(host[1].to_string());
            }

            let data = regex.replace_all(minions_data.as_str(), "").into_owned();

            trace!("deleted_minions: {:#?}", failed);

            for minion in failed {
                minions.insert(
                    minion.clone(),
                    host::Host {
                        hostname: minion,
                        status: host::HostStatus::DeletedMinion,
                        ..host::Host::default()
                    },
                );
            }

            data
        }
    };

    let value: Value = serde_json::from_str(minions_data.as_str())
        .chain_err(|| "can not convert minions data to minions")?;

    let mut parsed_minions =
        parse_minions_from_json(&value).chain_err(|| "can not convert json value to minions")?;

    minions.append(&mut parsed_minions);

    trace!("minions: {:#?}", minions);

    Ok(minions)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Retcode {
    Success,
    Failure,
}

impl Retcode {
    fn is_failure(&self) -> bool {
        self == &Retcode::Failure
    }
}

#[cfg(test)]
mod test_retcode {
    use Retcode;

    #[test]
    fn from_success() {
        assert_eq!(Retcode::Success, 0.into())
    }

    #[test]
    fn from_failure() {
        for i in 1..10 {
            assert_eq!(Retcode::Failure, i.into())
        }
    }
}

impl Default for Retcode {
    fn default() -> Retcode {
        Retcode::Failure
    }
}

impl From<u64> for Retcode {
    fn from(input: u64) -> Self {
        match input {
            0 => Retcode::Success,
            _ => Retcode::Failure,
        }
    }
}

fn parse_minions_from_json(
    json_value: &Value,
) -> std::result::Result<DataMap<String, host::Host>, errors::Error> {
    let mut minions: DataMap<String, host::Host> = DataMap::default();

    for (hostid, values) in json_value.as_object().unwrap().iter() {
        debug!("hostid: {:#?}", hostid);
        trace!("values: {:#?}", values);
        let hostid = hostid.to_owned();

        let mut host = host::Host {
            hostname: hostid.clone(),
            ..host::Host::default()
        };

        if values.get("ret") == None {
            debug!("going the single host path when parsing");

            match *values {
                Value::Object(ref r) => {
                    if r.is_empty() {
                        debug!("ret value is empty");
                        host.status = host::HostStatus::RetValueObjectIsEmpty;
                        minions.insert(hostid, host);
                        continue;
                    }

                    host.data = Some(values.clone());
                    host.status = host::HostStatus::Success;
                    minions.insert(hostid, host);
                    continue;
                }
                _ => {
                    debug!("type of values: {:#?}", *values);

                    host.status = host::HostStatus::RetValueNotObject;
                    minions.insert(hostid, host);
                    continue;
                }
            }
        }

        let ret_code: Retcode = match values.get("retcode") {
            Some(o) => match o.as_u64() {
                Some(v) => v.into(),
                None => {
                    host.status = host::HostStatus::ReturnCodeNotNumber;
                    minions.insert(hostid, host);
                    continue;
                }
            },
            None => {
                host.status = host::HostStatus::NoReturnCode;
                minions.insert(hostid, host);
                continue;
            }
        };

        if ret_code.is_failure() {
            host.status = host::HostStatus::RetCodeWasNotNull;
            minions.insert(hostid, host);
            continue;
        }

        let ret = match values.get("ret") {
            None => {
                host.status = host::HostStatus::RetValueIsNone;
                minions.insert(hostid, host);
                continue;
            }
            Some(r) => r,
        };

        match *ret {
            Value::Object(ref r) => {
                if r.is_empty() {
                    host.status = host::HostStatus::RetValueObjectIsEmpty;
                    minions.insert(hostid, host);
                    continue;
                }

                host.data = Some(values.clone());
                host.status = host::HostStatus::Success;
                minions.insert(hostid, host);
                continue;
            }
            _ => {
                debug!("type of ret: {:#?}", *ret);

                host.status = host::HostStatus::RetValueNotObject;
                minions.insert(hostid, host);
                continue;
            }
        }
    }

    Ok(minions)
}

fn serialize_minions(
    minions: DataMap<String, host::Host>,
    grainsdir: &PathBuf,
) -> Result<(), Error> {
    fs::create_dir_all(&grainsdir)
        .chain_err(|| "can not create grainsdir for writing minions json")?;

    let mut failed_log = {
        let mut fail_log_path = grainsdir.clone();
        fail_log_path.push("failed_minions.log");

        File::create(
            fail_log_path
                .to_str()
                .ok_or("can not convert fail_log_path to str")?,
        )
        .chain_err(|| "can not create file for writing failed_minions log")?
    };

    for (hostid, data) in minions {
        if data.status != host::HostStatus::Success {
            let message = format!(
                "host {} did not succedd. failed with status {:?}",
                hostid, data.status
            );
            warn!("{}", message);
            failed_log
                .write(format!("{}\n", message).as_bytes())
                .chain_err(|| "can not write message to failed_log file")?;

            continue;
        }

        let mut data_path = grainsdir.clone();
        data_path.push(format!("{}.json", hostid));

        let mut file = File::create(
            data_path
                .to_str()
                .ok_or("can not convert data_path to str")?,
        )
        .chain_err(|| "can not create file for writing minion data")?;

        let mut data_map = DataMap::default();
        data_map.insert(hostid, data.data);

        file.write(
            serde_json::to_string_pretty(&data_map)
                .chain_err(|| "can not convert minion data to json")?
                .as_bytes(),
        )
        .chain_err(|| "can not write json data to file")?;
    }

    Ok(())
}
