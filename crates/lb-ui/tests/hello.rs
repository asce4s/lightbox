use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext as _, TestAppContext, px};
use lb_ui::{HELLO_TEXT, ids, open_hello_window};

#[gpui_kit::test]
fn ui_hello_window_renders_label(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _view) = cx.update(open_hello_window).unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let label = window.find(ids::HELLO_LABEL);
        assert!(label.visible());
        assert_eq!(label.label(), Some(HELLO_TEXT));
        let size = label.bounds().size;
        assert!(size.height == px(32.), "{size:?}");
        assert!(size.width > px(0.), "{size:?}");
    })
    .unwrap();
}
