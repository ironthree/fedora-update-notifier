use std::cmp::PartialEq;
use std::convert::TryFrom;
use std::fs::read_to_string;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use bodhi::data::*;
use bodhi::BodhiServiceBuilder;

use notify_rust::Notification;

use serde_derive::Deserialize;

#[derive(Debug, PartialEq)]
struct NVR<'a> {
    n: &'a str,
    v: &'a str,
    r: &'a str,
}

#[derive(Debug, Deserialize)]
struct FedoraConfig {
    #[serde(rename(deserialize = "FAS"))]
    fas: FASConfig,
    #[serde(rename(deserialize = "fedora-update-notifier"))]
    fedora_update_notifier: FUNConfig,
}

#[derive(Debug, Deserialize)]
struct FASConfig {
    username: String,
}

#[derive(Debug, Deserialize)]
struct FUNConfig {
    interests: Vec<String>,
}

fn parse_nevra(nevra: &str) -> Result<(&str, &str, &str, &str, &str), String> {
    let mut nevr_a: Vec<&str> = nevra.rsplitn(2, '.').collect();

    if nevr_a.len() != 2 {
        return Err(format!("Unexpected error when parsing NEVRAs: {}", nevra));
    }

    // rsplitn returns things in reverse order
    let a = nevr_a.remove(0);
    let nevr = nevr_a.remove(0);

    let mut n_ev_r: Vec<&str> = nevr.rsplitn(3, '-').collect();

    if n_ev_r.len() != 3 {
        return Err(format!("Unexpected error when parsing NEVRAs: {}", nevr));
    }

    // rsplitn returns things in reverse order
    let r = n_ev_r.remove(0);
    let ev = n_ev_r.remove(0);
    let n = n_ev_r.remove(0);

    let (e, v) = if ev.contains(':') {
        let mut e_v: Vec<&str> = ev.split(':').collect();
        let e = e_v.remove(0);
        let v = e_v.remove(0);
        (e, v)
    } else {
        ("0", ev)
    };

    Ok((n, e, v, r, a))
}

fn parse_filename(nevrax: &str) -> Result<(&str, &str, &str, &str, &str), String> {
    let mut nevra_x: Vec<&str> = nevrax.rsplitn(2, '.').collect();

    if nevra_x.len() != 2 {
        return Err(format!(
            "Unexpected error when parsing dnf output: {}",
            nevrax
        ));
    }

    // rsplitn returns things in reverse order
    let _x = nevra_x.remove(0);
    let nevra = nevra_x.remove(0);

    let (n, e, v, r, a) = parse_nevra(nevra)?;
    Ok((n, e, v, r, a))
}

fn parse_nvr(nvr: &str) -> Result<(&str, &str, &str), String> {
    let mut n_v_r: Vec<&str> = nvr.rsplitn(3, '-').collect();

    if n_v_r.len() != 3 {
        return Err(format!("Unexpected error when parsing NEVRAs: {}", nvr));
    }

    // rsplitn returns things in reverse order
    let r = n_v_r.remove(0);
    let v = n_v_r.remove(0);
    let n = n_v_r.remove(0);

    Ok((n, v, r))
}

fn get_config() -> Result<FedoraConfig, String> {
    let home = match dirs::home_dir() {
        Some(path) => path,
        None => {
            return Err(String::from("Unable to determine $HOME."));
        }
    };

    let config_path = home.join(".config/fedora.toml");

    let config_str = match read_to_string(&config_path) {
        Ok(string) => string,
        Err(_) => {
            return Err(String::from(
                "Unable to read configuration file from ~/.config/fedora.toml",
            ));
        }
    };

    let config: FedoraConfig = match toml::from_str(&config_str) {
        Ok(config) => config,
        Err(_) => {
            return Err(String::from(
                "Unable to parse configuration file from ~/.config/fedora.toml",
            ));
        }
    };

    Ok(config)
}

fn get_release() -> Result<String, String> {
    let output = match Command::new("rpm").arg("--eval").arg("%{fedora}").output() {
        Ok(output) => output,
        Err(error) => {
            return Err(format!("{}", error));
        }
    };

    match output.status.code() {
        Some(x) if x != 0 => {
            return Err(String::from("Failed to run rpm."));
        }
        Some(_) => {}
        None => {
            return Err(String::from("Failed to run rpm."));
        }
    };

    let release_num = match std::str::from_utf8(&output.stdout) {
        Ok(result) => result,
        Err(error) => {
            return Err(format!("{}", error));
        }
    }
    .trim();

    let release = format!("F{}", release_num);

    Ok(release)
}

fn main() -> Result<(), String> {
    let app = clap::App::new("fedora-update-notifier")
        .arg(
            clap::Arg::with_name("username")
                .long("username")
                .value_name("username")
                .takes_value(true)
                .help("FAS user name"),
        )
        .arg(
            clap::Arg::with_name("interests")
                .required(false)
                .takes_value(true)
                .multiple(true)
                .help("interesting packages to check pending updates for"),
        )
        .about(
            r#"
If no arguments are specified on the command line, they will be read
from ~/.config/fedora.toml.

This config file is expected to be in this format:

    [FAS]
    username = "FAS_USERNAME"

    [fedora-update-notifier]
    interests = ["package1", "package2"]
"#,
        );

    let matches = app.get_matches();

    let config = get_config();

    let mut username: Option<String> = None;
    let mut interests: Option<Vec<String>> = None;

    if let Ok(config) = config {
        username = Some(config.fas.username);
        interests = Some(config.fedora_update_notifier.interests)
    }

    let cli_username = matches.value_of("username");
    let cli_interests: Option<Vec<&str>> = match matches.values_of("interests") {
        Some(values) => Some(values.collect()),
        None => None,
    };

    if let Some(cli_username) = cli_username {
        username = Some(cli_username.to_owned());
    }

    if let Some(cli_interests) = cli_interests {
        let mut strings: Vec<String> = cli_interests.into_iter().map(|e| e.to_owned()).collect();

        // don't override interests, but append
        match &mut interests {
            None => interests = Some(strings),
            Some(nonempty) => nonempty.append(&mut strings),
        }
    }

    let username = match username {
        Some(username) => username,
        None => {
            return Err(String::from("No FAS username was specified."));
        }
    };

    let interests = match interests {
        Some(interests) => interests,
        None => {
            return Err(String::from("No interests were specified."));
        }
    };

    // query rpm for current release
    let release = get_release()?;

    // query dnf for installed packages
    let output = match Command::new("dnf")
        .arg("--quiet")
        .arg("repoquery")
        .arg("--cacheonly")
        .arg("--installed")
        .arg("--source")
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return Err(format!("{}", error));
        }
    };

    match output.status.code() {
        Some(x) if x != 0 => {
            return Err(String::from("Failed to query dnf."));
        }
        Some(_) => {}
        None => {
            return Err(String::from("Failed to query dnf."));
        }
    };

    let installed = match std::str::from_utf8(&output.stdout) {
        Ok(result) => result,
        Err(error) => {
            return Err(format!("{}", error));
        }
    };

    let lines: Vec<&str> = installed.trim().split('\n').collect();

    let mut packages: Vec<NVR> = Vec::new();
    for line in lines {
        let (n, _, v, r, _) = parse_filename(line)?;
        packages.push(NVR { n, v, r });
    }

    // query bodhi for packages in updates-testing
    let bodhi = match BodhiServiceBuilder::default().build() {
        Ok(bodhi) => bodhi,
        Err(error) => {
            return Err(format!("{}", error));
        }
    };

    let query = bodhi::query::UpdateQuery::new()
        .releases(TryFrom::try_from(release.as_ref())?)
        .content_type(ContentType::RPM)
        .status(UpdateStatus::Testing);

    let updates = match query.query(&bodhi) {
        Ok(updates) => updates,
        Err(error) => {
            return Err(format!("{}", error));
        }
    };

    // filter out updates created by the current user
    let updates: Vec<Update> = updates
        .into_iter()
        .filter(|update| update.user.name != username)
        .collect();

    // filter out updates that were already commented on
    let mut relevant_updates: Vec<&Update> = Vec::new();
    for update in &updates {
        if let Some(comments) = &update.comments {
            let mut commented = false;

            for comment in comments {
                if comment.user.name == username {
                    commented = true;
                }
            }

            if !commented {
                relevant_updates.push(update);
            }
        } else {
            relevant_updates.push(update);
        }
    }

    // filter out updates for packages that are not installed
    let mut installed_updates: Vec<&Update> = Vec::new();
    for update in &relevant_updates {
        let mut nvrs: Vec<NVR> = Vec::new();

        for build in &update.builds {
            let (n, v, r) = parse_nvr(&build.nvr)?;
            nvrs.push(NVR { n, v, r });
        }

        for nvr in nvrs {
            if packages.contains(&nvr) {
                installed_updates.push(&update);
            }
        }
    }

    // collect relevant packages
    let mut installed_packages: Vec<&str> = Vec::new();
    for update in &installed_updates {
        for build in &update.builds {
            let (n, _, _) = parse_nvr(&build.nvr)?;
            installed_packages.push(n);
        }
    }

    // sort and remove duplicates
    installed_packages.sort();
    installed_packages.dedup_by(|a, b| a == b);

    println!();
    if !installed_packages.is_empty() {
        // construct update URL
        let feedback_url = format!(
            "https://bodhi.fedoraproject.org/updates/?release={}&status=testing&packages={}",
            release,
            installed_packages.join(",")
        );

        // send notification for updates that are ready for feedback
        Notification::new()
            .summary("Installed updates are ready for feedback")
            .body(&feedback_url)
            .icon("dialog-information")
            .show()
            .expect("Unable to send desktop notification.");

        println!("Installed updates are ready for feedback:");
        for installed_package in installed_packages {
            println!("- {}", installed_package);
        }
        println!("Feedback URL: {}", &feedback_url);
    } else {
        println!("No updates for installed packages are waiting for feedback.");
    }

    if interests.is_empty() {
        return Ok(());
    }

    // check if there are updates for "interesting" packages that aren't installed yet
    let mut pending_updates: Vec<&Update> = Vec::new();
    for update in &relevant_updates {
        let mut pending_nvrs: Vec<NVR> = Vec::new();

        for build in &update.builds {
            let (n, v, r) = parse_nvr(&build.nvr)?;
            pending_nvrs.push(NVR { n, v, r });
        }

        for nvr in &pending_nvrs {
            if packages.contains(&nvr) {
                // update is already installed, skip it
                continue;
            } else {
                // check if the package is installed, but not the pending update
                let mut is_installed: bool = false;
                for pending_nvr in &pending_nvrs {
                    for package in &packages {
                        if pending_nvr.n == package.n {
                            is_installed = true;
                        }
                    }
                }

                // check if the package is interesting
                let mut is_interesting: bool = false;
                for pending_nvr in &pending_nvrs {
                    for interest in &interests {
                        if interest == pending_nvr.n {
                            is_interesting = true;
                        }
                    }
                }

                if is_installed && is_interesting {
                    pending_updates.push(update);
                } else {
                    continue;
                }
            };
        }
    }

    // deduplicate pending updates
    pending_updates.sort_by(|a, b| a.alias.cmp(&b.alias));
    pending_updates.dedup_by(|a, b| a.alias == b.alias);

    if !interests.is_empty() && !pending_updates.is_empty() {
        println!();

        // construct interesting URL
        let interesting_url = format!(
            "https://bodhi.fedoraproject.org/updates/?release={}&status=testing&packages={}",
            release,
            &interests.join(",")
        );

        // don't clobber the DBus notification server
        sleep(Duration::from_secs(1));

        Notification::new()
            .summary("Updates for interesting packages are available for testing.")
            .body(&interesting_url)
            .icon("dialog-information")
            .show()
            .unwrap();

        println!("Updates for interesting packages are available for testing:");
        for pending_update in pending_updates {
            let builds: Vec<&str> = pending_update
                .builds
                .iter()
                .map(|b| b.nvr.as_ref())
                .collect();
            println!("- {}", &pending_update.alias);
            for build in builds {
                println!("  - {}", build);
            }
        }

        println!("Install the relevant updates with:");
        println!("sudo dnf upgrade --enablerepo=updates-testing --advisory=UPDATE_TITLE");
    } else if !interests.is_empty() {
        println!();
        println!("No updates for interesting packages are available.");
    }

    Ok(())
}
