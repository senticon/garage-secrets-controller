use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-env-changed=GSC_GIT_HASH");
    println!("cargo:rerun-if-env-changed=GSC_VERSION");
    println!("cargo:rerun-if-env-changed=GSC_PR_NUMBER");
    println!("cargo:rerun-if-env-changed=GITHUB_EVENT_NAME");
    println!("cargo:rerun-if-env-changed=GITHUB_REF");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_TYPE");

    let git_hash = git_hash();
    let version = explicit_version()
        .or_else(github_tag_version)
        .unwrap_or_else(|| match pr_number() {
            Some(pr_number) => format!("PR-{pr_number}-{git_hash}"),
            None => git_hash,
        });

    println!("cargo:rustc-env=BUILD_VERSION={version}");
    println!("cargo:rustc-env=BUILD_DATE={}", build_date());
}

fn explicit_version() -> Option<String> {
    env::var("GSC_VERSION")
        .ok()
        .filter(|value| !value.is_empty())
}

fn git_hash() -> String {
    env::var("GSC_GIT_HASH")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| command_output("git", &["rev-parse", "--short", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_string())
}

fn github_tag_version() -> Option<String> {
    if env::var("GITHUB_REF_TYPE").ok().as_deref() != Some("tag") {
        return None;
    }

    env::var("GITHUB_REF_NAME")
        .ok()
        .filter(|value| !value.is_empty())
}

fn pr_number() -> Option<String> {
    env::var("GSC_PR_NUMBER")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if env::var("GITHUB_EVENT_NAME").ok().as_deref() != Some("pull_request") {
                return None;
            }

            env::var("GITHUB_REF").ok().and_then(|value| {
                value
                    .strip_prefix("refs/pull/")?
                    .split('/')
                    .next()
                    .map(str::to_string)
            })
        })
}

fn build_date() -> String {
    command_output("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_else(|| "unknown".to_string())
}

fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}
