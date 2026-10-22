# craft-platform

Small platform lifecycle services for Craft applications. Application state, settings, palettes,
document policy and native command routing remain in their current owners. Defaults add no
third-party dependencies. Enable only the services a host uses.

| Feature | Shared responsibility |
| --- | --- |
| default | Typed dialog outcomes; no backend or toolkit. |
| `dialogs` | Reserve waiting workers before eager UI-thread native picker creation; serial requests, failure delivery and completion wakeups. The backend factory belongs to the app. |
| `egui` | Combine with `dialogs` for the deferred-close guard. |


## Native dialogs

`dialogs::wait_prepared(create, deliver)` reserves the worker first. Call it on the native UI thread.
`create` must call `pick_file`, `save_file`, or the equivalent backend method **before returning its
future**. Putting that call inside an async block delays native sheet creation until the worker
polls it. Worker reservation failure never opens the sheet. No live picker has a timeout.

`NativeDialogs<Request, Answer>` provides an optional serial queue. Pump it on the event-loop
thread before consuming answers and after code that can request a picker. Replies become visible
only after the pump releases transport ownership, so a continuation can enqueue the next picker.
Dropping a stale reply does not release the actual picker. A host-specific preflight is optional;
it runs off-thread and must be bounded. Unknown capability permits a picker. Proven absence may
request an application-owned fallback. Custom save-format sheets and portal choices remain intact.

`Outcome::Selected`, explicit `Cancelled`, `Failed` and `Indeterminate` have different evidence.
An rfd `None` is `Indeterminate`; it cannot prove user cancellation rather than backend failure.
Disconnected workers are failures. Apps own localization, document identity/revision checks, filters,
actual save receipts, chained continuations and the decision to close.

Retain the actual parent window through queued requests, preflight, the native picker AND any
unconsumed application continuation. `DeferredClose::update` cancels close during pending work and
returns a request to retry the application's normal close policy afterward. It never grants
permission to discard work. Run those guards before `craft_quit::on_close_request` in `App::logic`.

## Integration

Enable only the services a host uses. Applications retain palettes, settings, document identity,
close policy, filters, names, MIME types and native command routing. Existing craft-settings,
craft-atomic, craft-print, craft-docpath and craft-quit remain canonical.
