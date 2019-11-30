use std::cmp::PartialEq;
use std::convert::TryFrom;
use std::fs::read_to_string;
use std::process::Command;

use bodhi::BodhiServiceBuilder;
use bodhi::data::*;

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
    username: String,
}

fn parse_nevra(nevra: &str) -> Result<(&str, &str, &str, &str, &str), String> {
    let mut nevr_a: Vec<&str> = nevra.rsplitn(2, ".").collect();

    if nevr_a.len() != 2 {
        return Err(format!("Unexpected error when parsing NEVRAs: {}", nevra));
    }

    // rsplitn returns things in reverse order
    let a = nevr_a.remove(0);
    let nevr = nevr_a.remove(0);

    let mut n_ev_r: Vec<&str> = nevr.rsplitn(3, "-").collect();

    if n_ev_r.len() != 3 {
        return Err(format!("Unexpected error when parsing NEVRAs: {}", nevr));
    }

    // rsplitn returns things in reverse order
    let r = n_ev_r.remove(0);
    let ev = n_ev_r.remove(0);
    let n = n_ev_r.remove(0);

    let (e, v) = if ev.contains(":") {
        let mut e_v: Vec<&str> = ev.split(":").collect();
        let e = e_v.remove(0);
        let v = e_v.remove(0);
        (e, v)
    } else {
        ("0", ev)
    };

    Ok((n, e, v, r, a))
}

fn parse_filename(nevrax: &str) -> Result<(&str, &str, &str, &str, &str), String> {
    let mut nevra_x: Vec<&str> = nevrax.rsplitn(2, ".").collect();

    if nevra_x.len() != 2 {
        return Err(format!("Unexpected error when parsing dnf output: {}", nevrax));
    }

    // rsplitn returns things in reverse order
    let _x = nevra_x.remove(0);
    let nevra = nevra_x.remove(0);

    let (n, e, v, r, a) = parse_nevra(nevra)?;
    Ok((n, e, v, r, a))
}

fn parse_nvr(nvr: &str) -> Result<(&str, &str, &str), String> {
    let mut n_v_r: Vec<&str> = nvr.rsplitn(3, "-").collect();

    if n_v_r.len() != 3 {
        return Err(format!("Unexpected error when parsing NEVRAs: {}", nvr));
    }

    // rsplitn returns things in reverse order
    let r = n_v_r.remove(0);
    let v = n_v_r.remove(0);
    let n = n_v_r.remove(0);

    Ok((n, v, r))
}

fn main() -> Result<(), String> {
    let home = match dirs::home_dir() {
        Some(path) => path,
        None => { return Err(String::from("Unable to determine $HOME.")); }
    };

    let config_path = home.join(".config/fedora.toml");

    let config_str = match read_to_string(&config_path) {
        Ok(string) => string,
        Err(_) => { return Err(String::from("Unable to read configuration file from ~/.config/fedora.toml")); }
    };

    let config: FedoraConfig = match toml::from_str(&config_str) {
        Ok(config) => config,
        Err(_) => { return Err(String::from("Unable to parse configuration file from ~/.config/fedora.toml")); }
    };

    let username: &str = config.username.as_ref();

    let output = match Command::new("rpm")
        .arg("--eval")
        .arg("%{fedora}")
        .output() {
        Ok(output) => output,
        Err(error) => { return Err(format!("{}", error)); }
    };

    match output.status.code() {
        Some(x) if x != 0 => { return Err(String::from("Failed to run rpm.")); }
        Some(_) => {}
        None => { return Err(String::from("Failed to run rpm.")); }
    };

    let release_num = match std::str::from_utf8(&output.stdout) {
        Ok(result) => result,
        Err(error) => { return Err(format!("{}", error)); }
    }.trim();

    let release: &str = &format!("F{}", release_num);

    // query dnf for installed packages
    let output = match Command::new("dnf")
        .arg("--quiet")
        .arg("repoquery")
        .arg("--cacheonly")
        .arg("--installed")
        .arg("--source")
        .output() {
        Ok(output) => output,
        Err(error) => { return Err(format!("{}", error)); }
    };

    match output.status.code() {
        Some(x) if x != 0 => { return Err(String::from("Failed to query dnf.")); }
        Some(_) => {}
        None => { return Err(String::from("Failed to query dnf.")); }
    };

    let installed = match std::str::from_utf8(&output.stdout) {
        Ok(result) => result,
        Err(error) => { return Err(format!("{}", error)); }
    };

    let lines: Vec<&str> = installed.trim().split("\n").collect();

    let mut packages: Vec<NVR> = Vec::new();
    for line in lines {
        let (n, _, v, r, _) = parse_filename(line)?;
        packages.push(NVR { n, v, r });
    }

    // query bodhi for packages in updates-testing
    let bodhi = match BodhiServiceBuilder::default().build() {
        Ok(bodhi) => bodhi,
        Err(error) => { return Err(format!("{}", error)); }
    };

    let query = bodhi::query::UpdateQuery::new()
        .releases(TryFrom::try_from(release)?)
        .content_type(ContentType::RPM)
        .status(UpdateStatus::Testing);

    let updates = match query.query(&bodhi) {
        Ok(updates) => updates,
        Err(error) => { return Err(format!("{}", error)); }
    };

    // filter out updates created by the current user
    let updates: Vec<Update> = updates.into_iter().filter(
        |update| update.user.name != username
    ).collect();

    // filter out updates that were already commented on
    let mut relevant_updates: Vec<&Update> = Vec::new();
    for update in &updates {
        if let Some(comments) = &update.comments {
            let mut commented = false;

            for comment in comments {
                if &comment.user.name == username {
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

    // construct update URL
    let url = format!(
        "https://bodhi.fedoraproject.org/updates/?release={}&status=testing&packages={}",
        release,
        installed_packages.join(",")
    );

    // send notification
    Notification::new()
        .summary("Installed updates are ready for feedback")
        .body(&url)
        .icon("dialog-information")
        .show().unwrap();

    Ok(())
}

