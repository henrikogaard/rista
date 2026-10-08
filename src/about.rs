use objc2::{runtime::AnyObject, AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSAboutPanelOptionApplicationIcon, NSAboutPanelOptionApplicationName,
    NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionKey, NSAboutPanelOptionVersion,
    NSApplication, NSImage,
};
use objc2_foundation::{NSData, NSDictionary, NSString};

pub(crate) fn show() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let icon_data = NSData::with_bytes(include_bytes!("../public/icon.png"));
    let Some(icon) = NSImage::initWithData(NSImage::alloc(), &icon_data) else {
        return;
    };

    let application_name = NSString::from_str("Rísta");
    let application_version = NSString::from_str(env!("CARGO_PKG_VERSION"));
    let version = NSString::from_str("");
    let copyright = NSString::from_str("Copyright © Henrik Øgård");
    let keys: [&NSAboutPanelOptionKey; 5] = unsafe {
        [
            NSAboutPanelOptionApplicationName,
            NSAboutPanelOptionApplicationVersion,
            NSAboutPanelOptionVersion,
            objc2_foundation::ns_string!("Copyright"),
            NSAboutPanelOptionApplicationIcon,
        ]
    };
    let values: [&AnyObject; 5] = [
        application_name.as_ref(),
        application_version.as_ref(),
        version.as_ref(),
        copyright.as_ref(),
        icon.as_ref(),
    ];
    let options = NSDictionary::from_slices(&keys, &values);

    unsafe {
        NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanelWithOptions(&options);
    }
}
