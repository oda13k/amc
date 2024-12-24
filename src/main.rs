// Copyright Olaru Alexandru
// Licensed under the MIT license

use core::time;
use std::{env, process::exit};
use xcb::randr;

#[macro_use]
mod amc;

pub struct DeferCall<DeferFunc: FnMut()> {
    defer_func: DeferFunc,
}

impl<DeferFunc: FnMut()> Drop for DeferCall<DeferFunc> {
    fn drop(&mut self) {
        (self.defer_func)();
    }
}

macro_rules! defer {
    ($e:expr) => {
        let _defer_call = DeferCall {
            defer_func: || -> () {
                $e;
            },
        };
    };
}

macro_rules! println_error {
    ($msg:expr) => {{
        println!("Error: {}", $msg);
    }};
}

macro_rules! die {
    ($msg:expr) => {{
        println_error!($msg);
        exit(1);
    }};
}

#[derive(Debug)]
pub struct MonitorSetup {
    configs: Vec<amc::MonitorConfig>,
}

fn amc_apply_best_setup_for_mons(
    xstack: &amc::XCBStack,
    mons: &Vec<amc::Monitor>,
    mon_setups: &Vec<MonitorSetup>,
) -> amc::Result<()> {
    let mut setup_hit = false;
    let mut configs_changed = false;

    let mut screen_w = 0;
    let mut screen_h = 0;
    /* These two variables fucking anger me; upset me on a personal level even.
    Why does randr NEED to know the screen w and h in MILLIMETERS. What
    happens if I have empty gaps in between the monitors, how do I calculate these
    values?? What is the canonical size IN FUCKING MILLIMETERS of an arbitrarily sized
    undisplayable region of pixels? I will not even bother looking at the xrandr source
    to see what bullshit they came up with because I know it will only add to my
    growing hatred for the dumpsterfire that is X11 */
    let mut screen_w_mm = 0;
    let mut screen_h_mm = 0;

    for setup in mon_setups {
        match setup
            .configs
            .iter()
            .map(|conf| mons.iter().find(|mon| mon.id == conf.id).is_some())
            .reduce(|acc, e| acc && e)
        {
            Some(true) => (),
            Some(false) => continue,
            None => continue,
        };

        setup_hit = true;

        for conf in &setup.configs {
            let mon = mons.iter().find(|mon| mon.id == conf.id).unwrap();

            let config_applied = mon.apply_config(xstack, conf)?;

            configs_changed = configs_changed || config_applied;

            match conf.rot {
                randr::Rotation::ROTATE_0 | randr::Rotation::ROTATE_180 => {
                    screen_w = std::cmp::max(screen_w, (conf.x as u16) + mon.w);
                    screen_h = std::cmp::max(screen_h, (conf.y as u16) + mon.h);
                }
                randr::Rotation::ROTATE_90 | randr::Rotation::ROTATE_270 => {
                    screen_w = std::cmp::max(screen_w, (conf.x as u16) + mon.h);
                    screen_h = std::cmp::max(screen_h, (conf.y as u16) + mon.w);
                }
                _ => unreachable!(),
            }

            screen_w_mm += mon.w_mm;
            screen_h_mm += mon.h_mm;
        }

        break;
    }

    if !setup_hit {
        /* If no setup matched what's connected, we mirror each display.
         * We could rig each display to a single crtc if they
         * are the same size, mode, etc but I can't be fucked to check all of that.
         */
        for mon in mons {
            let config_applied = mon.apply_config(
                xstack,
                &amc::MonitorConfig {
                    id: mon.id,
                    x: 0,
                    y: 0,
                    rot: randr::Rotation::ROTATE_0,
                },
            );

            configs_changed = configs_changed || config_applied?;
            screen_w = std::cmp::max(screen_w, mon.w);
            screen_h = std::cmp::max(screen_h, mon.h);
            screen_w_mm = std::cmp::max(screen_w_mm, mon.w_mm);
            screen_h_mm = std::cmp::max(screen_h_mm, mon.h_mm);
        }
    }

    if configs_changed {
        xstack.conn.send_request(&randr::SetScreenSize {
            window: *xstack.root_window,
            width: screen_w,
            height: screen_h,
            mm_width: screen_w_mm,
            mm_height: screen_h_mm,
        });
    }

    Ok(())
}

fn amc_parse_setup_from_conf_str(file_content: &String) -> amc::Result<MonitorSetup> {
    let mut mon_setup = MonitorSetup {
        configs: Vec::new(),
    };

    for (line_n, mut line) in file_content.lines().map(|l| l.to_string()).enumerate() {
        line.retain(|c| !c.is_whitespace());

        if line.is_empty() || line.chars().nth(0).unwrap() == '#' {
            continue;
        }

        let (lhs, rhs): (&str, &str) = match line.split_once('=') {
            Some((lhs, rhs)) => {
                if lhs.is_empty() || rhs.is_empty() {
                    return Err(format_args!("Invalid config at line {}", line_n)
                        .to_string()
                        .into());
                }

                (lhs, rhs)
            }
            None => {
                return Err(format_args!("Invalid config at line {}", line_n)
                    .to_string()
                    .into())
            }
        };

        let mon_id = match u32::from_str_radix(lhs, 16) {
            Ok(x) => x,
            Err(_) => {
                return Err(format_args!("Invalid monitor id at line {}", line_n)
                    .to_string()
                    .into())
            }
        };

        let (x, y, rot) = match rhs.split_once(',') {
            Some((xy, rot)) => {
                if xy.is_empty() {
                    return Err(format_args!("Missing monitor position at line {}", line_n)
                        .to_string()
                        .into());
                }

                if rot.is_empty() {
                    return Err(format_args!("Missing monitor rotation at line {}", line_n)
                        .to_string()
                        .into());
                }

                let rot_n = match rot.parse::<u16>() {
                    Ok(x) => x,
                    Err(_) => {
                        return Err(format_args!("Invalid monitor rotation at line {} (rotation can only have the following values: 0, 90, 180, 270)", line_n)
                            .to_string()
                            .into());
                    }
                };

                if rot_n != 0 && rot_n != 90 && rot_n != 180 && rot_n != 270 {
                    return Err(format_args!("Invalid monitor rotation at line {} (rotation can only have the following values: 0, 90, 180, 270)", line_n)
                            .to_string()
                            .into());
                }

                let (x, y) = match xy.split_once('x') {
                    Some((x, y)) => {
                        if x.is_empty() || y.is_empty() {
                            return Err(format_args!(
                                "Invalid monitor position at line {}",
                                line_n
                            )
                            .to_string()
                            .into());
                        }

                        let x_n = match x.parse::<u16>() {
                            Ok(x) => x,
                            Err(_) => {
                                return Err(format_args!(
                                    "Invalid monitor position at line {}",
                                    line_n
                                )
                                .to_string()
                                .into());
                            }
                        };

                        let y_n = match y.parse::<u16>() {
                            Ok(x) => x,
                            Err(_) => {
                                return Err(format_args!(
                                    "Invalid monitor position at line {}",
                                    line_n
                                )
                                .to_string()
                                .into());
                            }
                        };

                        (x_n, y_n)
                    }
                    None => {
                        return Err(format_args!("Invalid config at line {}", line_n)
                            .to_string()
                            .into())
                    }
                };

                (x, y, rot_n)
            }
            None => {
                return Err(format_args!("Invalid config at line {}", line_n)
                    .to_string()
                    .into())
            }
        };

        mon_setup.configs.push(amc::MonitorConfig {
            id: mon_id,
            x: x as i16,
            y: y as i16,
            rot: match rot {
                0 => randr::Rotation::ROTATE_0,
                90 => randr::Rotation::ROTATE_90,
                180 => randr::Rotation::ROTATE_180,
                270 => randr::Rotation::ROTATE_270,
                _ => unreachable!(),
            },
        });
    }

    Ok(mon_setup)
}

fn amc_read_setups_from_dir(dir: &String) -> amc::Result<Vec<MonitorSetup>> {
    match std::fs::exists(dir) {
        Ok(true) => (),
        Ok(false) => {
            if let Err(err) = std::fs::create_dir(dir) {
                return Err(
                    format_args!("Could not create config dir '{}'\n  {}", dir, err)
                        .to_string()
                        .into(),
                );
            };
        }
        Err(err) => {
            return Err(format_args!(
                "Could not check existance of config dir '{}'\n  {}",
                dir, err
            )
            .to_string()
            .into())
        }
    };

    let files = match std::fs::read_dir(dir) {
        Ok(files) => files.collect::<Result<Vec<_>, std::io::Error>>()?, // why tf can this even be Err?
        Err(err) => {
            return Err(
                format_args!("Could not list files in dir '{}'\n  {}", dir, err)
                    .to_string()
                    .into(),
            )
        }
    };

    let mut setups = Vec::<MonitorSetup>::new();

    for file in files {
        let file_content = match std::fs::read_to_string(file.path()) {
            Ok(x) => x,
            Err(err) => {
                return Err(format_args!(
                    "Could not read config file '{}'\n  {}",
                    file.path().to_str().unwrap(),
                    err
                )
                .to_string()
                .into())
            }
        };

        match amc_parse_setup_from_conf_str(&file_content) {
            Ok(setup) => {
                setups.push(setup);
            }
            Err(err) => {
                return Err(format_args!(
                    "Could not parse config file '{}'\n  {}",
                    file.path().to_str().unwrap(),
                    err
                )
                .to_string()
                .into())
            }
        }
    }

    Ok(setups)
}

fn help(bin_path: &String, error: Option<&String>) {
    if error.is_some() {
        println!("{}: {}.", bin_path, error.unwrap());
        println!("Try '{} --help' for more information.", bin_path);
        return;
    }

    println!("Usage: {} [options]", bin_path);
    println!("(Connector name independent) Auto Monitor Configurator for X11");
    println!("");
    println!("options:");
    println!("  -h, --help            Print this message and exit");
    println!("  -c, --config-dir      Path to config dir from where to grab monitor configs (By default $XDG_CONFIG_HOME/amc)");
    println!("  -p, --print-monitors  Print information on all connected monitors (helpful for configuring)");
    println!("  -d, --daemon          Start amc as a daemon");
    println!("\nConfiguration:");
    println!("  amc matches and configures monitors based on 'setups'. Setups define a certain configuration of \n  one or more monitors. Configuration only happens in an integral fashion, meaning that either a \n  setup's configuration exactly matches what is plugged in and everything gets configured as \n  specified in the config file, or nothing gets matched and we set a defeault config for every monitor \n  that's plugged in. The default is placing each monitor at 0x0, no rotation & best available mode \n  (mirroring each other).");
    println!("\n  For each monitor setup you have, you'll have to create a separate config file detailing that setup's \n  configuration and place it inside amc's config dir.");
    println!("\n  Configs must end in '.conf' and have the following structure:");
    println!("    <monitor id> = <x>x<y>, <rotation degrees>");
    println!("    ... Repeat that for every monitor in that setup ...");
    println!(
        "\n  You can get the id of each connected monitor in parenthesis by running '{} -p'.",
        bin_path,
    );
    println!("\n  Rotation can only be: 0, 90, 180 or 270");
    println!("\nWhy:");
    println!("  Because my fuckass Thinkpad Dock Gen 2 randomly changes it's connector names even if \n  the physical connections haven't been touched. This tool configures monitors based \n  on their EDIDs and doesn't care about which ports they are plugged into.");
    println!("  Also because I wanted to learn some rust");
    println!("\nLicense:");
    println!("  MIT do whatever you want, idc");
}

fn main() {
    let args = env::args().collect::<Vec<String>>();

    let mut config_dir = match env::var("XDG_CONFIG_HOME") {
        Ok(xdg_config_home) => format_args!("{}/amc", xdg_config_home).to_string(),
        Err(_) => match env::var("HOME") {
            Ok(home) => format_args!("{}/.config/amc", home).to_string(),
            Err(_) => panic!("Couldn't get user's config dir"), // We could try $USER
        },
    };

    let mut daemon = false;
    let mut print_monitors = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                help(&args[0], None);
                exit(0);
            }
            "-c" | "--config" => {
                if i + 1 >= args.len() {
                    help(
                        &args[0],
                        Some(
                            &format_args!("Option '{}' requires an argument", args[i]).to_string(),
                        ),
                    );
                    exit(1);
                }

                i += 1;
                config_dir = args[i].to_string();
            }
            "-p" | "--print-monitors" => {
                print_monitors = true;
            }
            "-d" | "--daemon" => {
                daemon = true;
            }
            invalid_arg => {
                help(
                    &args[0],
                    Some(&format_args!("Invalid option -- '{}'", invalid_arg).to_string()),
                );
                exit(1);
            }
        }

        i += 1;
    }

    let (conn, screen_num) = match xcb::Connection::connect(None) {
        Ok(x) => x,
        Err(_) => die!("Could not connect to X server"),
    };
    let root_screen = conn.get_setup().roots().nth(screen_num as usize).unwrap();
    let root_window = root_screen.root();

    let xstack = amc::XCBStack {
        conn: &conn,
        root_screen: root_screen,
        root_window: &root_window,
    };

    if print_monitors {
        let mons = match amc::Monitor::get_all_connected(&xstack) {
            Ok(x) => x,
            Err(err) => die!(err),
        };

        if mons.len() == 0 {
            println!("No connected monitors");
        } else {
            println!("Connected monitors:");
            for mon in mons {
                println!("  {} ({:x})", mon.name, mon.id)
            }
        }
        exit(0);
    }

    let mon_setups = match amc_read_setups_from_dir(&config_dir) {
        Ok(x) => x,
        Err(err) => die!(err),
    };

    // FIXME: this crashes when i run amc at dwm startup?
    // if mon_setups.len() == 0 {
    //     println!(
    //         "No setups were found in '{}'",
    //         std::path::absolute(config_dir).unwrap().to_str().unwrap()
    //     );
    // } else {
    //     println!("Found {} setup(s)", mon_setups.len());
    // }

    // We printed the stuff the user might want to see, so we can detach
    if daemon {
        match unsafe { libc::fork() } {
            0 => (), // child
            -1 => die!("Failed to daemonize"),
            _pid => exit(0), // parent, we're done
        }

        /* We are now the daemonized child, close all stdout/in handles and become
        our own session's leader to become independent from our parent. */
        unsafe {
            libc::setsid();
            libc::close(libc::STDIN_FILENO);
            libc::close(libc::STDOUT_FILENO);
            libc::close(libc::STDERR_FILENO);
        }
    }

    loop {
        /* I can't get the XRRScreenChangeNotify event to fire (if that's even the right one),
        so polling it is */
        defer!(std::thread::sleep(time::Duration::from_secs(3)));

        let mons = match amc::Monitor::get_all_connected(&xstack) {
            Ok(x) => x,
            Err(err) => {
                if conn.has_error().is_err() {
                    die!("X connection closed");
                }

                println_error!(err);
                continue;
            }
        };

        if let Err(err) = amc_apply_best_setup_for_mons(&xstack, &mons, &mon_setups) {
            if conn.has_error().is_err() {
                die!("X connection closed");
            }

            println_error!(err);
        };
    }
}
