# get_grains

## Usage
```
get_grains 1.0.0
Alexander Thaller <alexander.thaller@trivago.com>
Get the grains for all the minions connected to the saltmaster and save them in JSON files

USAGE:
    get_grains [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help
            Prints help information

    -V, --version
            Prints version information


OPTIONS:
    -D, --grainsdir <grainsdir>
            folder to save the grainfiles in [default: grains]

    -l, --loglevel <level>
            Loglevel to run under [default: info]  [values: trace, debug, info, warn, error]


SUBCOMMANDS:
    help
            Prints this message or the help of the given subcommand(s)

    read_file
            read salt grains from a file

    run_salt
            run salt command to get grains and parse that
```
