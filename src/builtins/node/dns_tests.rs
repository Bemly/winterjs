    use super::*;

    #[test]
    fn dns_node_codes() {
        reset_for_tests();
        assert_eq!(node_code_for("DNS error: NoRecordsFound(..)"), "ENODATA");
        assert_eq!(node_code_for("nxdomain: no such host"), "ENOTFOUND");
        assert_eq!(node_code_for("request timed out"), "ETIMEOUT");
        assert_eq!(node_code_for("refused by server"), "EREFUSED");
        assert_eq!(node_code_for("servfail"), "SERVFAIL");
        assert_eq!(node_code_for("totally unknown blah"), "ENOTFOUND");
        reset_for_tests();
    }

    #[test]
    fn dns_server_addr_parse() {
        // 10f 定制路径：规范形解析（JS 已 canonicalize，此处为 backstop）
        let v4 = "127.0.0.1".parse::<IpAddr>().unwrap();
        let v6 = "::1".parse::<IpAddr>().unwrap();
        assert_eq!(
            parse_server_addr("127.0.0.1"),
            Some(DnsServer { ip: v4, port: 53 })
        );
        assert_eq!(
            parse_server_addr("127.0.0.1:5353"),
            Some(DnsServer { ip: v4, port: 5353 })
        );
        // port 0 即默认端口（Node 口径）
        assert_eq!(
            parse_server_addr("127.0.0.1:0"),
            Some(DnsServer { ip: v4, port: 53 })
        );
        assert_eq!(
            parse_server_addr("[::1]"),
            Some(DnsServer { ip: v6, port: 53 })
        );
        assert_eq!(
            parse_server_addr("[::1]:5353"),
            Some(DnsServer { ip: v6, port: 5353 })
        );
        assert_eq!(
            parse_server_addr("2001:4860:4860::8888"),
            Some(DnsServer {
                ip: "2001:4860:4860::8888".parse().unwrap(),
                port: 53
            })
        );
        assert_eq!(parse_server_addr("foobar"), None);
        assert_eq!(parse_server_addr("127.0.0.1:va"), None);
        assert_eq!(parse_server_addr("127.0.0.1:"), None);
        assert_eq!(parse_server_addr("[::1"), None);
        assert_eq!(parse_server_addr(""), None);
    }

    #[test]
    fn dns_udp_fail_codes() {
        // 10f：失败→Node 码映射（stub 坏包/超时/拒连三件）
        assert_eq!(udp_fail_code(&UdpFail::Timeout).0, "ETIMEOUT");
        assert_eq!(udp_fail_code(&UdpFail::BadResponse("decode: x".into())).0, "EBADRESP");
        assert_eq!(
            udp_fail_code(&UdpFail::Io("connection refused".into())).0,
            "ECONNREFUSED"
        );
        assert_eq!(udp_fail_code(&UdpFail::Io("boom".into())).0, "EAI_AGAIN");
    }

    #[test]
    fn dns_custom_no_servers() {
        // 空 servers 即失败（无线程/I-O，纯逻辑）
        use hickory_resolver::proto::rr::RecordType;
        let err = query_custom_sync(
            &[],
            "example.org",
            RecordType::A,
            std::time::Duration::from_millis(10),
            1,
            std::time::Duration::ZERO,
        )
        .unwrap_err();
        assert_eq!(err.0, "ENOTFOUND");
    }

    #[test]
    fn dns_servers_roundtrip() {
        reset_for_tests();
        let first = effective_servers();
        assert!(!first.is_empty());
        *servers_slot().lock() = Some(vec!["1.1.1.1".to_string()]);
        assert_eq!(effective_servers(), vec!["1.1.1.1".to_string()]);
        *order_slot().lock() = "ipv4first".to_string();
        assert_eq!(order_slot().lock().as_str(), "ipv4first");
        reset_for_tests();
    }

    #[test]
    fn dns_trim_dot() {
        assert_eq!(trim_dot("localhost."), "localhost");
        assert_eq!(trim_dot("a.b."), "a.b");
        assert_eq!(trim_dot("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn dns_resolver_config_builds() {
        use hickory_resolver::config::ResolverConfig;
        let cfg = resolver_config_for(&["127.0.0.1".to_string(), "nope".to_string()]);
        assert_eq!(cfg.name_servers().len(), 1);
        let empty = ResolverConfig::default();
        assert!(empty.name_servers().is_empty());
    }
