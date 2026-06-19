use egui::{Color32, Pos2};
use uwu::state::{AppState, CanvasObject, CanvasShape, CanvasShapeType};

#[test]
fn test_app_flow_history() {
    let mut app_state = AppState::default();

    // Verify initial state
    assert_eq!(app_state.canvas.objects.len(), 0);

    // Simulate drawing a shape
    let dummy_shape = CanvasObject::Shape(CanvasShape {
        shape_type: CanvasShapeType::Rectangle,
        pos: Pos2::new(10.0, 10.0),
        size: 50.0,
        color: Color32::RED,
    });

    app_state.canvas.objects.push(dummy_shape.clone());
    app_state.history.save_add_object(0, dummy_shape);

    assert_eq!(app_state.canvas.objects.len(), 1);

    // Perform an undo
    let success = app_state.history.undo(&mut app_state.canvas);
    assert!(success);
    assert_eq!(app_state.canvas.objects.len(), 0);

    // Perform a redo
    let success = app_state.history.redo(&mut app_state.canvas);
    assert!(success);
    assert_eq!(app_state.canvas.objects.len(), 1);
}
