//! The relay (docs/multiplayer-checklist.md item 2), on this machine:
//! rooms and their codes, a full room, a wrong code or game, a player
//! dropping and coming back, and the limits that keep it safe to leave
//! running.

use argus_net::Link;
use argus_relay::{ws, Client, Limits, Relay};
use std::io::{Read, Write};
use std::time::Duration;

fn relay(limits: Limits) -> String {
    let h = Relay::bind("127.0.0.1:0", limits).unwrap().spawn();
    format!("ws://{}", h.addr())
}

/// Wait for what `c` is sent, up to a second.
fn got(c: &mut Client, n: usize) -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    for _ in 0..50 {
        all.extend(c.wait(20));
        if all.len() >= n {
            break;
        }
    }
    all
}

fn present(c: &mut Client, want: &[u8]) {
    for _ in 0..50 {
        if c.present() == want {
            return;
        }
        c.wait(20);
    }
    panic!("present {:?}, wanted {want:?}", c.present());
}

#[test]
fn a_room_has_a_code_and_passes_each_players_bytes_to_the_others() {
    let url = relay(Limits::default());
    let mut a = Client::host(&url, 3, 7).unwrap();
    let code = a.code().to_string();
    assert_eq!(code.len(), 5);
    assert!(
        code.chars()
            .all(|c| "ABCDEFGHJKMNPQRSTUVWXYZ23456789".contains(c)),
        "{code}"
    );
    assert_eq!((a.slot(), a.players()), (1, 3));
    // A code is read in any case.
    let mut b = Client::join(&url, &code.to_lowercase(), 7).unwrap();
    let mut c = Client::join(&url, &code, 7).unwrap();
    assert_eq!((b.slot(), c.slot()), (2, 3));
    present(&mut a, &[1, 2, 3]);
    a.send(b"from one".to_vec());
    b.send(b"from two".to_vec());
    let mut seen_c = got(&mut c, 2);
    seen_c.sort();
    assert_eq!(seen_c, vec![b"from one".to_vec(), b"from two".to_vec()]);
    assert_eq!(
        got(&mut a, 1),
        vec![b"from two".to_vec()],
        "not its own back"
    );
    assert_eq!(got(&mut b, 1), vec![b"from one".to_vec()]);
}

#[test]
fn a_full_room_a_wrong_code_and_another_game_are_refused() {
    let url = relay(Limits::default());
    let a = Client::host(&url, 2, 7).unwrap();
    let _b = Client::join(&url, a.code(), 7).unwrap();
    let err = |r: std::io::Result<Client>| r.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(err(Client::join(&url, a.code(), 7)).contains("is full"));
    assert!(err(Client::join(&url, "ZZZZZ", 7)).contains("no room ZZZZZ"));
    let h = Client::host(&url, 2, 7).unwrap();
    assert!(err(Client::join(&url, h.code(), 8)).contains("another game"));
    assert!(err(Client::host(&url, 9, 7)).contains("2 to 4 players"));
}

#[test]
fn a_player_who_drops_comes_back_to_its_slot() {
    let url = relay(Limits::default());
    let mut a = Client::host(&url, 2, 7).unwrap();
    let mut b = Client::join(&url, a.code(), 7).unwrap();
    present(&mut a, &[1, 2]);
    let (code, seat) = (a.code().to_string(), b.seat());
    b.drop_connection();
    present(&mut a, &[1]);
    // Nobody else takes the slot while it is held; its seat does.
    let err = Client::join(&url, &code, 7).err().unwrap().to_string();
    assert!(err.contains("full"), "{err}");
    let wrong = Client::rejoin(&url, &code, 7, seat ^ 2).err().unwrap();
    assert!(wrong.to_string().contains("seat"), "{wrong}");
    let mut b = Client::rejoin(&url, &code, 7, seat).unwrap();
    assert_eq!(b.slot(), 2);
    present(&mut a, &[1, 2]);
    a.send(b"again".to_vec());
    assert_eq!(got(&mut b, 1), vec![b"again".to_vec()]);
}

#[test]
fn a_room_nobody_comes_back_to_goes() {
    let h = Relay::bind(
        "127.0.0.1:0",
        Limits {
            empty: Duration::from_millis(200),
            rooms: 1,
            ..Limits::default()
        },
    )
    .unwrap()
    .spawn();
    let url = format!("ws://{}", h.addr());
    let mut a = Client::host(&url, 2, 7).unwrap();
    let err = Client::host(&url, 2, 7).err().unwrap().to_string();
    assert!(err.contains("all the rooms"), "{err}");
    a.drop_connection();
    std::thread::sleep(Duration::from_millis(400));
    // The next host sweeps it away and takes its place.
    let b = Client::host(&url, 2, 7).unwrap();
    assert_ne!(b.code(), a.code());
    assert_eq!(h.counts().0, 1);
}

#[test]
fn a_player_breaking_the_limits_is_dropped() {
    let url = relay(Limits {
        message: 1000,
        rate: 50.0,
        burst: 20.0,
        idle: Duration::from_millis(300),
        ..Limits::default()
    });
    // Too big.
    let mut a = Client::host(&url, 2, 7).unwrap();
    a.send(vec![0; 2000]);
    got(&mut a, 1);
    assert!(
        a.closed().is_some_and(|w| w.contains("over 1000 bytes")),
        "{:?}",
        a.closed()
    );
    // Too fast.
    let mut b = Client::host(&url, 2, 7).unwrap();
    for _ in 0..100 {
        b.send(vec![1]);
    }
    got(&mut b, 1);
    assert!(
        b.closed().is_some_and(|w| w.contains("messages a second")),
        "{:?}",
        b.closed()
    );
    // Too quiet: no ping, nothing sent.
    let mut c = Client::host(&url, 2, 7).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    got(&mut c, 1);
    assert!(
        c.closed().is_some_and(|w| w.contains("silent")),
        "{:?}",
        c.closed()
    );
}

#[test]
fn what_is_not_a_relay_message_is_refused() {
    let h = Relay::bind("127.0.0.1:0", Limits::default())
        .unwrap()
        .spawn();
    // A plain web request is told what this is.
    let mut s = std::net::TcpStream::connect(h.addr()).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut text = String::new();
    let _ = s.read_to_string(&mut text);
    assert!(
        text.starts_with("HTTP/1.1 400") && text.contains("Argus relay"),
        "{text}"
    );
    // Bytes that are no message.
    let (mut s, mut r) = ws::connect(&format!("ws://{}", h.addr())).unwrap();
    s.write_all(&ws::frame(ws::op::BINARY, &[99, 1, 2], Some([5, 6, 7, 8])))
        .unwrap();
    let (_, _, data) = ws::read_frame(&mut r, 1 << 16).unwrap();
    let m = argus_relay::Down::decode(&data).unwrap();
    assert!(
        matches!(&m, argus_relay::Down::Refused(w) if w.contains("cannot read")),
        "{m:?}"
    );
    // Sending before joining.
    let (mut s, mut r) = ws::connect(&format!("ws://{}", h.addr())).unwrap();
    let send = argus_relay::Up::Send(vec![1]).encode();
    s.write_all(&ws::frame(ws::op::BINARY, &send, Some([1, 1, 1, 1])))
        .unwrap();
    let (_, _, data) = ws::read_frame(&mut r, 1 << 16).unwrap();
    assert_eq!(
        argus_relay::Down::decode(&data).unwrap(),
        argus_relay::Down::Refused("join a room first".into())
    );
    // Each refused connection closes, after a moment for the player's last bytes.
    drop((s, r));
    for _ in 0..100 {
        if h.counts() == (0, 0) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("connections left open: {:?}", h.counts());
}

#[test]
fn a_relay_made_slow_holds_each_message_its_lag() {
    let url = relay(Limits {
        lag: Duration::from_millis(80),
        jitter: Duration::from_millis(20),
        ..Limits::default()
    });
    let mut a = Client::host(&url, 2, 7).unwrap();
    let mut b = Client::join(&url, a.code(), 7).unwrap();
    present(&mut a, &[1, 2]);
    for i in 0..5u8 {
        let t = std::time::Instant::now();
        a.send(vec![i]);
        let got = got(&mut b, 1);
        let took = t.elapsed();
        assert_eq!(got, vec![vec![i]]);
        assert!(took >= Duration::from_millis(80), "{took:?}");
        assert!(took < Duration::from_millis(400), "{took:?}");
    }
}
