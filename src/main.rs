mod templates;
use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;

use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin};
use bevy::prelude::*;
use bevy::render::{
    render_asset::RenderAssetUsages,
    render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bevy_simple_text_input::{TextInput, TextInputPlugin, TextInputSubmitEvent, TextInputTextFont};
use chrono::Utc;
use templates::Template;

const BRUSH_THICKNESS: u32 = 3;
const BRUSH_COLOR: Color = Color::linear_rgb(255.0, 255.0, 255.0);
const BOARD_COLOR: Color = Color::linear_rgb(0.0, 0.0, 0.0);
const RESAMPLE_TARGET_POINTS: usize = 64;
const SCALE_SIZE: f32 = 100.0;

#[derive(Resource)]
struct DrawingBoard(Handle<Image>);

#[derive(Resource)]
struct IsTyping(bool);

#[derive(Resource)]
struct OverAButton(bool);

#[derive(Resource)]
struct ResampledPoints(Vec<Vec2>);
#[derive(Resource)]
struct StrokeTemplates(HashMap<String, HashSet<Template>>);

#[derive(Component)]
struct ResultText;

#[derive(PartialEq)]
enum DrawMoment {
    Idle,
    InputEnded,
    InputBegan(Vec2),
    Held(Vec2),
}

#[derive(Resource)]
struct DrawState(DrawMoment);

#[derive(Resource)]
struct BrushEnabled(bool); // DISABLE FOR BETTER PERFORMANCE SINCE THEN IT DOES NOT HAVE TO DO 360*BRUSH_THICKNESS ITERATIONS

#[derive(Component)]
struct ToggleBrushButton;

#[derive(Component)]
struct AddGestureButton;

fn resample(total_length: f32, candidate_points: &Vec<Vec2>) -> Vec<Vec2> {
    let mut resampled_points = Vec::with_capacity(RESAMPLE_TARGET_POINTS);
    resampled_points.push(candidate_points[0]);

    if candidate_points.len() > 1 {
        /*
         distance squared would be faster but using it leads to inaccuracies with the lerping and alpha;
         sqrting the alpha gives lesser points for some reason;
        */

        let increment = total_length / (RESAMPLE_TARGET_POINTS) as f32;
        let mut accumulated_distance = 0.0;
        let mut previous_point = candidate_points[0];

        for i in 1..candidate_points.len() {
            let current_point = candidate_points[i];
            let mut segment_distance = previous_point.distance(current_point);

            while accumulated_distance + segment_distance >= increment
                && resampled_points.len() < RESAMPLE_TARGET_POINTS
            {
                let alpha = (increment - accumulated_distance) / segment_distance;
                let new_point = previous_point.lerp(current_point, alpha);

                resampled_points.push(new_point);

                previous_point = new_point;
                accumulated_distance = 0.0;
                segment_distance = previous_point.distance(current_point);
            }

            accumulated_distance += segment_distance;
            previous_point = current_point;
        }
    }

    resampled_points
}

fn get_centroid(points: &Vec<Vec2>) -> Vec2 {
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    for point in points.iter() {
        sum_x += point.x;
        sum_y += point.y;
    }
    sum_x /= points.len() as f32;
    sum_y /= points.len() as f32;
    Vec2::new(sum_x, sum_y)
}

fn rotate_about_centroid(points: &mut Vec<Vec2>) {
    let centroid = get_centroid(&points);
    let indicative_angle = ops::atan2(centroid.y - points[0].y, centroid.x - points[0].x) + PI;
    // rotation of a point about origin formula was x = x'cosx + y'sinx and for y you add pi/2
    let cos = ops::cos(indicative_angle);
    let sin = ops::sin(indicative_angle);
    for point in points.iter_mut() {
        let x_ = point.x - centroid.x;
        let y_ = point.y - centroid.y;
        point.x = x_ * cos + y_ * sin + centroid.x;
        point.y = y_ * cos - x_ * sin + centroid.y;
    }
}

fn scale_and_translate(points: &mut Vec<Vec2>) {
    // GET BOUNDING BOX CO-ORDS
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for point in points.iter() {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    let b_width = max_x - min_x;
    let b_height = max_y - min_y;

    // SCALING (SCALING MESSES UP STRAIGHT LINES)
    for point in points.iter_mut() {
        point.x = point.x * (SCALE_SIZE / b_width);
        point.y = point.y * (SCALE_SIZE / b_height);
    }

    // TRANSLATE TO ORIGIN (offset is for debugging purposes)
    let centroid = get_centroid(points);
    for point in points.iter_mut() {
        point.x += -centroid.x;
        point.y += -centroid.y;
    }
}

fn recognize(points: &Vec<Vec2>, templates: Res<StrokeTemplates>) -> (String, f32) {
    let mut nearest_distance_squared = f32::MAX;
    let mut nearest_name = "not recognized";
    
    for unistroke in templates.0.iter() {
        for template in unistroke.1.iter() {
            let distance = distance_at_best_angle(points, &template.0);
            if distance < nearest_distance_squared {
                nearest_distance_squared = distance;
                nearest_name = unistroke.0;
            }
        }
    }

    (nearest_name.to_string(), nearest_distance_squared)
}

fn distance_at_best_angle(points: &Vec<Vec2>, template_points: &[Vec2; RESAMPLE_TARGET_POINTS]) -> f32 {
    // follows the golden-section search algorithm
    const DELTA_THETA: f32 = 0.03490658503; // 2 deg in rads
    const INVERSE_PHI: f32 = 0.61803398875;

    let mut theta_max = 0.78539816339; // 45 deg in rads
    let mut theta_min = -0.78539816339; // 45 deg in rads
    let mut x1 = INVERSE_PHI * theta_min + (1.0 - INVERSE_PHI) * theta_max;
    let mut f1 = distance_at_angle(points, template_points, x1);
    let mut x2 = (1.0 - INVERSE_PHI) * theta_min + INVERSE_PHI * theta_max;
    let mut f2 = distance_at_angle(points, template_points, x2);

    while (theta_max - theta_min).abs() > DELTA_THETA {
        if f1 < f2 {
            theta_max = x2;
            x2 = x1;
            f2 = f1;
            x1 = INVERSE_PHI * theta_min + (1.0 - INVERSE_PHI) * theta_max;
            f1 = distance_at_angle(points, template_points, x1)
        } else {
            theta_min = x1;
            x1 = x2;
            f1 = f2;
            x2 = (1.0 - INVERSE_PHI) * theta_min + INVERSE_PHI * theta_max;
            f2 = distance_at_angle(points, template_points, x2)
        }
    }

    f32::min(f1, f2)
}

fn distance_at_angle(points: &Vec<Vec2>, template_points: &[Vec2; RESAMPLE_TARGET_POINTS], theta: f32) -> f32 {
    let mut rotated_points = Vec::with_capacity(points.len());
    let centroid = get_centroid(points);
    let cos = ops::cos(theta);
    let sin = ops::sin(theta);
    for point in points.iter() {
        let x_ = point.x - centroid.x;
        let y_ = point.y - centroid.y;
        rotated_points.push(Vec2::new(
            x_ * cos + y_ * sin + centroid.x,
            y_ * cos - x_ * sin + centroid.y,
        ));
    }
    let mut path_distance = 0.0;
    for index in 0..points.len() {
        // squared distance is quicker; dont really care about score
        let d = rotated_points[index].distance_squared(template_points[index]);
        path_distance += d;
    }
    path_distance / (points.len() as f32).powi(2)
}

fn reset_board(window_size: Vec2, board: &mut Image, resize: bool) {
    if resize {
        board.resize(Extent3d {
            width: window_size.x as u32,
            height: window_size.y as u32,
            depth_or_array_layers: 1,
        });
    }

    for x in 0..(window_size.x as u32) {
        for y in 0..(window_size.y as u32) {
            board.set_color_at(x, y, BOARD_COLOR).unwrap_or(());
        }
    }
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            TextInputPlugin,
            FpsOverlayPlugin {
                config: FpsOverlayConfig {
                    text_config: TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    text_color: Color::linear_rgb(0.0, 255.0, 0.0),
                    enabled: true,
                },
            },
        ))
        .add_systems(Startup, (setup_window, spawn))
        .add_systems(
            Update,
            (toggle_brush, handle_adding_gestures, draw_state_handler, draw, textbox_input_listener).chain(),
        )
        .insert_resource(IsTyping(false))
        .insert_resource(OverAButton(false))
        .insert_resource(ResampledPoints(Vec::new()))
        .insert_resource(StrokeTemplates(templates::stroke_templates()))
        .insert_resource(DrawState(DrawMoment::Idle))
        .insert_resource(BrushEnabled(true))
        .run();
}

fn toggle_brush(
    mut brush_enabled: ResMut<BrushEnabled>,
    mut interaction_query: Query<
        (
            &Interaction,
            &mut BorderColor,
        ),
        (Changed<Interaction>, With<ToggleBrushButton>),
    >,
    mut text: Single<&mut Text, With<ToggleBrushButton>>,
) {
    for (interaction, mut border_color) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                brush_enabled.0 = !brush_enabled.0;
                border_color.0 = bevy::color::palettes::css::LIGHT_GREEN.into();
                text.0 = if brush_enabled.0 { format!("ON") } else { format!("OFF") };
            }
            _ => {
                text.0 = format!("Toggle Brush");
                border_color.0 = Color::WHITE;
            }
        }
    }
}

fn draw_state_handler(
    buttons: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    mut draw_state: ResMut<DrawState>,
    window: Single<&Window>,
) {
    if buttons.just_pressed(MouseButton::Left) {
        if let Some(x) = window.cursor_position() {
            draw_state.0 = DrawMoment::InputBegan(x);
        }
    } else if buttons.pressed(MouseButton::Left) {
        if let Some(x) = window.cursor_position() {
            draw_state.0 = DrawMoment::Held(x);
        }
    } else {
        for touch in touches.iter() {
            if touches.just_pressed(touch.id()) {
                draw_state.0 = DrawMoment::InputBegan(touch.position());
            } else {
                draw_state.0 = DrawMoment::Held(touch.position());
            }
            break;
        }
    }

    if buttons.just_released(MouseButton::Left) || touches.any_just_released() {
        draw_state.0 = DrawMoment::InputEnded;
    }
}

fn handle_adding_gestures(
    mut commands: Commands,
    mut typing: ResMut<IsTyping>,
    mut over_button: ResMut<OverAButton>,
    mut interaction_query: Query<
        (
            &Interaction,
            &mut BorderColor,
        ),
        (Changed<Interaction>, With<AddGestureButton>),
    >,
    result_text: Single<&Text, With<ResultText>>,
) {
    for (interaction, mut border_color) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                over_button.0 = true;
                border_color.0 = bevy::color::palettes::css::LIGHT_GREEN.into();
                if !result_text.0.is_empty() && !typing.0 {
                    typing.0 = true;
                    commands
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            bottom: Val::Px(300.0),
                            ..default()
                        })
                        .with_children(|parent| {
                            parent.spawn((
                                Node {
                                    width: Val::Px(200.0),
                                    border: UiRect::all(Val::Px(5.0)),
                                    padding: UiRect::all(Val::Px(5.0)),
                                    ..default()
                                },
                                BorderColor(BRUSH_COLOR),
                                TextInput,
                                TextInputTextFont(TextFont {
                                    font_size: 34.,
                                    ..default()
                                }),
                            ));
                        });
                }
            }
            _ => {
                border_color.0 = Color::WHITE;
            }
        }
    }
}

fn textbox_input_listener(
    mut events: EventReader<TextInputSubmitEvent>,
    mut typing: ResMut<IsTyping>,
    mut commands: Commands,
    resampled_points: Res<ResampledPoints>,
    mut custom_templates: ResMut<StrokeTemplates>,
    mut result_text: Single<&mut Text, With<ResultText>>,
) {
    for event in events.read() {
        let text = &event.value;
        
        if resampled_points.0.len() == RESAMPLE_TARGET_POINTS {
            if let Some(set) = custom_templates.0.get_mut(text) {
                set.insert(Template(resampled_points.0.to_owned().try_into().unwrap()));
            } else {
                custom_templates.0.insert(text.clone(), HashSet::from([Template(resampled_points.0.to_owned().try_into().unwrap())]));
            }
            result_text.0 = format!("{} gesture added!", text);
        } else {
            result_text.0 = format!("Gesture drawn has too little resampled points (< {})", RESAMPLE_TARGET_POINTS);
        }
        
        typing.0 = false;
        commands.entity(event.entity).despawn();
    }
}

fn fill_pixel(board: &mut Image, vec: Vec2, first_pixel: bool, brush_enabled: bool) {
    let thickness = if first_pixel { BRUSH_THICKNESS*2 } else { BRUSH_THICKNESS };
    if brush_enabled {
        for theta in 0..=360 {
            for delta_r in 0..=thickness {
                let x = vec.x + (delta_r as f32) * ops::cos((theta as f32).to_radians());
                let y = vec.y + (delta_r as f32) * ops::sin((theta as f32).to_radians());
                board
                    .set_color_at(x as u32, y as u32, BRUSH_COLOR)
                    .unwrap_or(()); // most likely the error would be an out_of_bounds so it i think im okay to ignore
            }
        }
    } else {
        board
            .set_color_at(vec.x as u32, vec.y as u32, BRUSH_COLOR)
            .unwrap_or(()); // most likely the error would be an out_of_bounds so it i think im okay to ignore
    }
}

fn draw(
    mut result_text: Single<&mut Text, With<ResultText>>,
    drawingboard: Res<DrawingBoard>,
    mut images: ResMut<Assets<Image>>,

    window: Single<&Window>,

    is_typing: Res<IsTyping>,
    mut over_button: ResMut<OverAButton>,
    custom_templates: Res<StrokeTemplates>,
    mut final_resampled_points: ResMut<ResampledPoints>,
    mut previous_pos: Local<Vec2>,
    mut candidate_points: Local<Vec<Vec2>>,
    mut total_length: Local<f32>,

    mut draw_state: ResMut<DrawState>,
    brush_enabled: Res<BrushEnabled>,
) {
    if is_typing.0 || over_button.0 {
        draw_state.0 = DrawMoment::Idle;
        over_button.0 = false;
        return;
    }
    if let DrawMoment::InputBegan(mouse_pos) = draw_state.0 {
        candidate_points.clear();
        *total_length = 0.0;
        result_text.0 = "".to_string();

        let board = images.get_mut(&drawingboard.0).expect("Board not found!!");
        reset_board(window.size(), board, true);

        fill_pixel(board, mouse_pos, true, brush_enabled.0);
        *previous_pos = mouse_pos;
        candidate_points.push(mouse_pos);
    } else if draw_state.0 == DrawMoment::InputEnded {
        let start_time = Utc::now();

        let mut resampled_points = resample(*total_length, &candidate_points);
        rotate_about_centroid(&mut resampled_points);
        scale_and_translate(&mut resampled_points);
        let (shape, _least_path_squared) = recognize(&resampled_points, custom_templates);

        let end_time = Utc::now();
        let elapsed_time = end_time.signed_duration_since(start_time);
        result_text.0 = format!(
            "{}\n{}.{} milliseconds",
            shape,
            elapsed_time.num_milliseconds(),
            elapsed_time.num_microseconds().get_or_insert_default()
        );
        final_resampled_points.0 = resampled_points;
        draw_state.0 = DrawMoment::Idle;
    } else if let DrawMoment::Held(mouse_pos) = draw_state.0 {
        let board = images.get_mut(&drawingboard.0).expect("Board not found!!");
        let delta = previous_pos.distance(mouse_pos);

        if delta > 6.0 {
            let num_steps = (delta / BRUSH_THICKNESS as f32).ceil() as u32;
            for step in 0..=num_steps {
                let alpha = step as f32 / num_steps as f32;
                let dv = previous_pos.lerp(mouse_pos, alpha);
                fill_pixel(board, dv, false, brush_enabled.0);
            }
        } else {
            fill_pixel(board, mouse_pos, false, brush_enabled.0);
        }

        candidate_points.push(mouse_pos);
        *total_length += delta;
        *previous_pos = mouse_pos;
    }
}
fn spawn(window: Single<&Window>, mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::linear_rgb(0.0, 255.0, 0.0)),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(0.0),
            ..default()
        },
        ResultText,
    ));
    commands.spawn((
        Text::new(
            "Misrecognized? 'Add' stroke as a gesture\n\n\n'Toggle Brush' for performance",
        ),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::linear_rgb(0.0, 255.0, 0.0)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(30.0),
            left: Val::Px(150.0),
            ..default()
        },
    ));
    
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::End,
            ..default()
        })
        .with_children(|parent| {
            parent
                .spawn((
                    Button,
                    Node {
                        width: Val::Px(140.0),
                        height: Val::Px(65.0),
                        border: UiRect::all(Val::Px(3.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor(Color::WHITE),
                    BorderRadius::MAX,
                    BackgroundColor(Color::srgb(0.15, 0.15, 0.15)),
                    ToggleBrushButton
                ))
                .with_child((
                    Text::new("Toggle Brush"),
                    TextFont {
                        font_size: 17.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.9, 0.9, 0.9)),
                    ToggleBrushButton
                ));
        });
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::End,
            bottom: Val::Px(80.0),
            ..default()
        })
        .with_children(|parent| {
            parent
                .spawn((
                    Button,
                    Node {
                        width: Val::Px(140.0),
                        height: Val::Px(65.0),
                        border: UiRect::all(Val::Px(3.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor(Color::WHITE),
                    BorderRadius::MAX,
                    BackgroundColor(Color::srgb(0.15, 0.15, 0.15)),
                    AddGestureButton
                ))
                .with_child((
                    Text::new("Add"),
                    TextFont {
                        font_size: 17.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.9, 0.9, 0.9)),
                ));
        });
    let image = Image::new_fill(
        Extent3d {
            width: window.size().x as u32,
            height: window.size().y as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &(BOARD_COLOR.to_srgba().to_u8_array()),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );

    let handle = images.add(image);
    commands.spawn(Sprite::from_image(handle.clone()));
    commands.insert_resource(DrawingBoard(handle));
}

fn setup_window(mut window: Single<&mut Window>) {
    window.title = String::from("$1 Unistroke Pattern Recognizer");
    window.position = WindowPosition::Centered(MonitorSelection::Current);
}
