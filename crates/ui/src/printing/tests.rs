use super::*;

fn queue(name: &str, default: bool) -> Printer {
    Printer { name: name.into(), default }
}

#[test]
fn discovery_preserves_choice_and_blocks_stale_or_removed_queues() {
    let ctx = egui::Context::default();
    let mut state = State::default();
    state.settings.printer = "Chosen".into();
    let discover: Discover = Box::new(|| {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(vec![queue("Default", true), queue("Chosen", false)]).unwrap();
        receiver
    });
    assert!(state.refresh(&discover));
    assert!(!state.ready());
    state.poll(&ctx);
    assert_eq!(state.settings.printer, "Chosen");
    assert!(state.ready());
    let empty: Discover = Box::new(|| {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(Vec::new()).unwrap();
        receiver
    });
    assert!(state.refresh(&empty));
    assert!(!state.ready());
    state.poll(&ctx);
    assert!(!state.ready());
    assert_eq!(state.message, Some(Feedback::NoPrinters));
    assert!(state.settings.printer.is_empty());
    assert!(state.refresh(&discover));
    state.poll(&ctx);
    assert_eq!(state.settings.printer, "Default");
}

#[test]
fn failed_discovery_cannot_reuse_a_previously_available_queue() {
    let mut state = State::default();
    state.printers.push(queue("Old", true));
    state.settings.printer = "Old".into();
    let discover: Discover = Box::new(|| std::sync::mpsc::channel().1);
    assert!(state.refresh(&discover));
    state.poll(&egui::Context::default());
    assert_eq!(state.message, Some(Feedback::DiscoveryFailed));
    assert!(!state.ready());
}

#[test]
fn active_jobs_keep_their_receiver_and_report_receipt_refusal_or_worker_failure() {
    let ctx = egui::Context::default();
    let mut state = State::default();
    state.printers.push(queue("Queue", true));
    state.settings.printer = "Queue".into();
    let (sender, receiver) = std::sync::mpsc::channel();
    assert!(state.begin(receiver).is_ok());
    assert!(!state.ready());
    let (other_sender, other_receiver) = std::sync::mpsc::channel();
    assert!(state.begin(other_receiver).is_err());
    drop(other_sender);
    state.poll(&ctx);
    assert!(state.busy());
    sender.send(Ok("Queue-42".into())).unwrap();
    state.poll(&ctx);
    assert_eq!(state.message, Some(Feedback::Submitted("Queue-42".into())));
    assert!(state.ready());
    let (sender, receiver) = std::sync::mpsc::channel();
    assert!(state.begin(receiver).is_ok());
    sender.send(Err("queue refused the job".into())).unwrap();
    state.poll(&ctx);
    assert_eq!(state.message, Some(Feedback::Failed("queue refused the job".into())));
    let (sender, receiver) = std::sync::mpsc::channel();
    assert!(state.begin(receiver).is_ok());
    drop(sender);
    state.poll(&ctx);
    assert_eq!(state.message, Some(Feedback::WorkerStopped));
    assert!(state.ready());
}

#[test]
fn discovery_and_controls_do_not_submit_a_job() {
    let mut state = State::default();
    let discover: Discover = Box::new(|| {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(vec![queue("Office", true)]).unwrap();
        receiver
    });
    let labels = Labels {
        printer: "Drucker",
        select_printer: "Wählen",
        refresh: "Aktualisieren",
        copies: "Kopien",
        printer_default: "Standard",
        duplex: "Seiten",
        one_sided: "Einseitig",
        long_edge: "Lange Kante",
        short_edge: "Kurze Kante",
        grayscale: "Graustufen",
    };
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| state.show(ui, egui::Id::new("print"), &discover, &labels));
    });
    assert_eq!(state.settings.printer, "Office");
    assert!(state.ready());
    assert!(!state.busy());
    assert_eq!(state.settings.copies, 1);
    assert_eq!(state.settings.duplex, None);
    assert!(!state.settings.grayscale);
}
