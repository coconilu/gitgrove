use super::*;

#[test]
fn desktop_support_does_not_enable_web_adapters() {
    let support = agent_open_support();
    for agent in [Agent::Codex, Agent::KimiDesktop] {
        assert_eq!(
            support.supports(agent),
            cfg!(any(windows, target_os = "macos"))
        );
    }
    for agent in [Agent::Kimi, Agent::Dsh] {
        assert_eq!(support.supports(agent), cfg!(windows));
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn web_adapters_reject_direct_backend_calls_before_opening_anything() {
    for agent in [Agent::Kimi, Agent::Dsh] {
        let error = open_in_agent("invalid path".into(), agent)
            .await
            .err()
            .unwrap();
        assert!(error.starts_with("当前平台暂不支持此工具入口"));
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn launch_services_preserves_directory_arguments_and_forwards_warm_launches() {
        let bundle = Path::new("/Applications/Kimi Code.app");
        for target in [
            "/tmp/example-project",
            "/tmp/中文目录 with spaces",
            "/tmp/project's \"quoted\" & $(echo unsafe); # worktree",
        ] {
            let target = Path::new(target);
            let codex = codex_macos_command(target);
            assert_eq!(codex.get_program(), "/usr/bin/open");
            assert_eq!(
                codex.get_args().collect::<Vec<_>>(),
                vec![
                    "-b".as_ref(),
                    "com.openai.codex".as_ref(),
                    target.as_os_str()
                ]
            );
            let kimi = kimi_desktop_command(bundle, target);
            assert_eq!(kimi.get_program(), "/usr/bin/open");
            let args: Vec<_> = kimi
                .get_args()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                args,
                vec![
                    "-n".to_string(),
                    "-a".to_string(),
                    bundle.display().to_string(),
                    "--args".to_string(),
                    format!("--workspace={}", target.display()),
                ]
            );
        }
    }

    #[test]
    fn incomplete_or_non_executable_bundles_are_not_installed() {
        let root = std::env::temp_dir().join(format!("gitgrove-bundle-test-{}", ulid::Ulid::generate()));
        let bundle = root.join("Kimi Code.app");
        let executable = bundle.join("Contents/MacOS/Kimi Code");
        assert!(!kimi_desktop_available(&bundle));
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        assert!(!kimi_desktop_available(&bundle));
        std::fs::write(&executable, "placeholder").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!kimi_desktop_available(&bundle));
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(kimi_desktop_available(&bundle));
        assert!(target_directory("relative-path").is_err());
        assert!(target_directory(executable.to_str().unwrap()).is_err());
        assert!(target_directory(root.join("missing").to_str().unwrap()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn launcher_failure_is_not_reported_as_a_sent_request() {
        let message = "应用启动失败";
        assert_eq!(
            run_macos_open(Command::new("/usr/bin/false"), message).await,
            Err(message.into())
        );
        assert_eq!(
            run_macos_open(Command::new("/nonexistent/gitgrove-launcher"), message).await,
            Err(message.into())
        );
        assert!(run_macos_open(Command::new("/usr/bin/true"), message)
            .await
            .is_ok());
    }
}

#[tokio::test]
#[ignore = "GUI acceptance: opens a desktop app; requires explicit target and tool"]
async fn desktop_open() {
    let path = std::env::var("GITGROVE_AGENT_TARGET")
        .expect("set GITGROVE_AGENT_TARGET to an existing absolute directory");
    let agent = match std::env::var("GITGROVE_AGENT_TOOL").as_deref() {
        Ok("codex") => Agent::Codex,
        Ok("kimidesktop") => Agent::KimiDesktop,
        _ => panic!("set GITGROVE_AGENT_TOOL=codex or kimidesktop"),
    };
    let receipt = open_in_agent(path, agent).await.unwrap();
    println!("{}", receipt.message);
}
