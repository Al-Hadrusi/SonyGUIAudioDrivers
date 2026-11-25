//! Desktop notifications (freedesktop D-Bus `org.freedesktop.Notifications`).

use headphones_control::battery_alert::Alert;

/// Show a low-battery notification without blocking the UI. Fails silently
/// when no notification service is running.
pub fn battery(device: &str, alert: Alert) {
    #[cfg(all(feature = "notifications", target_os = "linux"))]
    {
        let (summary, urgency) = match alert {
            Alert::Low(_) => ("Headphone battery low", notify_rust::Urgency::Normal),
            Alert::Critical(_) => ("Headphone battery very low", notify_rust::Urgency::Critical),
        };
        let level = match alert {
            Alert::Low(l) | Alert::Critical(l) => l,
        };
        let body = format!("{device}: {level}% left. Charge your headphones soon.");
        std::thread::spawn(move || {
            let result = notify_rust::Notification::new()
                .appname("Headphones Control")
                .summary(summary)
                .body(&body)
                .icon("headphones-control")
                .urgency(urgency)
                .show();
            if let Err(e) = result {
                eprintln!("Could not show notification: {e}");
            }
        });
    }
    #[cfg(not(all(feature = "notifications", target_os = "linux")))]
    {
        let _ = (device, alert);
    }
}
