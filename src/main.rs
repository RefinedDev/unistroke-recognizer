mod unistrokes;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::time::Instant;

use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin};
use bevy::input::mouse::{AccumulatedMouseMotion, MouseButtonInput};
use bevy::prelude::*;
use bevy::render::{
    render_asset::RenderAssetUsages,
    render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bevy_simple_text_input::{TextInput, TextInputPlugin, TextInputSubmitEvent, TextInputTextFont};

const BRUSH_ENABLED: bool = true; // DISABLE FOR BETTER PERFORMANCE SINCE THEN IT DOES NOT HAVE TO DO 360*BRUSH_THICKNESS ITERATIONS
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
struct ResampledPoints(Vec<Vec2>);
#[derive(Resource)]
struct StrokeTemplates(HashMap<String, Vec<Vec2>>);

#[derive(Component)]
struct ResultText;

#[derive(Default)]
struct M1Held(bool);

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
    for point in points.iter_mut() {
        let x_ = point.x - centroid.x;
        let y_ = point.y - centroid.y;
        point.x = x_ * ops::cos(indicative_angle) + y_ * ops::sin(indicative_angle) + centroid.x;
        point.y = y_ * ops::cos(indicative_angle) - x_ * ops::sin(indicative_angle) + centroid.y;
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
    
    for template in templates.0.iter() {
        let distance = distance_at_best_angle(points, template.1);
        if distance < nearest_distance_squared {
            nearest_distance_squared = distance;
            nearest_name = template.0;
        }
    }

    (nearest_name.to_string(), nearest_distance_squared)
}

fn distance_at_best_angle(points: &Vec<Vec2>, template_points: &Vec<Vec2>) -> f32 {
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

fn distance_at_angle(points: &Vec<Vec2>, template_points: &Vec<Vec2>, theta: f32) -> f32 {
    let mut rotated_points = Vec::with_capacity(points.len());
    let centroid = get_centroid(points);
    for point in points.iter() {
        let x_ = point.x - centroid.x;
        let y_ = point.y - centroid.y;
        rotated_points.push(Vec2::new(
            x_ * ops::cos(theta) + y_ * ops::sin(theta) + centroid.x,
            y_ * ops::cos(theta) - x_ * ops::sin(theta) + centroid.y,
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
            (draw, handle_adding_gestures, textbox_input_listener).chain(),
        )
        .insert_resource(IsTyping(false))
        .insert_resource(ResampledPoints(Vec::new()))
        .insert_resource(StrokeTemplates(unistrokes::stroke_templates()))
        // .insert_resource(M1Held(false))
        .run();
}

fn handle_adding_gestures(
    mut commands: Commands,
    mut typing: ResMut<IsTyping>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    result_text: Single<&Text, With<ResultText>>,
) {
    if keyboard_input.just_pressed(KeyCode::Space) && !result_text.0.is_empty() && !typing.0 {
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

        if let Some(_) = custom_templates.0.get(text) {
            result_text.0 = format!("{} gesture already exists!", text);
        } else {
            result_text.0 = format!("{} gesture added!", text);
            custom_templates.0.insert(text.clone(), resampled_points.0.clone());
            typing.0 = false;
            commands.entity(event.entity).despawn();
        }
    }
}

fn draw(
    mut result_text: Single<&mut Text, With<ResultText>>,
    drawingboard: Res<DrawingBoard>,
    mut images: ResMut<Assets<Image>>,

    window: Single<&Window>,

    is_typing: Res<IsTyping>,
    custom_templates: Res<StrokeTemplates>,
    mut final_resampled_points: ResMut<ResampledPoints>,
    mut previous_pos: Local<Vec2>,
    mut m1held: Local<M1Held>,
    mut candidate_points: Local<Vec<Vec2>>,
    mut total_length: Local<f32>,

    mouse_delta: Res<AccumulatedMouseMotion>,
    mut button_events: EventReader<MouseButtonInput>,
) {
    if is_typing.0 {
        return;
    }
    for button_event in button_events.read() {
        if button_event.button == MouseButton::Left {
            if m1held.0 == false && button_event.state.is_pressed() == true {
                // started drawing so clear stuff
                candidate_points.clear();
                *total_length = 0.0;
                *previous_pos = Vec2::ZERO;
                result_text.0 = "".to_string();

                let board = images.get_mut(&drawingboard.0).expect("Board not found!!");
                reset_board(window.size(), board, true);
            } else if m1held.0 == true && button_event.state.is_pressed() == false {
                // stopped drawing
                let now = Instant::now();

                let mut resampled_points = resample(*total_length, &candidate_points);
                rotate_about_centroid(&mut resampled_points);
                scale_and_translate(&mut resampled_points);

                let (shape, _least_path_squared) = recognize(&resampled_points, custom_templates);

                let elapsed_time = now.elapsed();
                result_text.0 = format!(
                    "{}\n{}.{} milliseconds",
                    shape,
                    elapsed_time.as_millis(),
                    elapsed_time.as_micros()
                );
                final_resampled_points.0 = resampled_points;
            }

            *m1held = M1Held(button_event.state.is_pressed());
            break;
        }
    }

    if m1held.0 {
        if let Some(mouse_pos) = window.cursor_position() {
            let mut fill_pixel = |vec: Vec2| {
                let board = images.get_mut(&drawingboard.0).expect("Board not found!!");
                if BRUSH_ENABLED {
                    for theta in 0..=360 {
                        for delta_r in 0..=BRUSH_THICKNESS {
                            let x_e =
                                vec.x + (delta_r as f32) * ops::cos((theta as f32).to_radians());
                            let y_e =
                                vec.y + (delta_r as f32) * ops::sin((theta as f32).to_radians());
                            board
                                .set_color_at(x_e as u32, y_e as u32, BRUSH_COLOR)
                                .unwrap_or(()); // most likely the error would be an out_of_bounds so it i think im okay to ignore
                        }
                    }
                } else {
                    board
                        .set_color_at(vec.x as u32, vec.y as u32, BRUSH_COLOR)
                        .unwrap_or(()); // most likely the error would be an out_of_bounds so it i think im okay to ignore
                }
            };

            if mouse_delta.delta.length_squared() > 36.0 && *previous_pos != Vec2::ZERO {
                let d = previous_pos.distance(mouse_pos);
                let num_steps = (d / BRUSH_THICKNESS as f32).ceil() as u32;
                for step in 0..=num_steps {
                    let alpha = step as f32 / num_steps as f32;
                    let dv = previous_pos.lerp(mouse_pos, alpha);
                    fill_pixel(dv);
                }
                *total_length += d;
            } else {
                fill_pixel(mouse_pos);
                if *previous_pos != Vec2::ZERO {
                    *total_length += previous_pos.distance(mouse_pos);
                }
            }

            candidate_points.push(mouse_pos);
            *previous_pos = mouse_pos;
        }
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
            "Make strokes on the canvas\nMisrecognized? Press SPACE to add unistroke as a gesture",
        ),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::linear_rgb(0.0, 255.0, 0.0)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(0.0),
            ..default()
        },
    ));
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
