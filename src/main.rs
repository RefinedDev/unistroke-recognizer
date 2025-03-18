use std::f32::consts::PI;

use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin};
use bevy::input::mouse::{AccumulatedMouseMotion, MouseButtonInput};
use bevy::prelude::*;
use bevy::render::{
    render_asset::RenderAssetUsages,
    render_resource::{Extent3d, TextureDimension, TextureFormat},
};

const BRUSH_ENABLED: bool = true; // DISABLE FOR BETTER PERFORMANCE SINCE THEN IT DOES NOT HAVE TO DO 360*BRUSH_THICKNESS ITERATIONS
const BRUSH_THICKNESS: u32 = 3;

const BOARD_COLOR: Color = Color::linear_rgb(0.0, 0.0, 0.0);
const DRAW_COLOR: Color = Color::linear_rgb(255.0, 255.0, 255.0);

#[derive(Resource)]
struct DrawingBoard(Handle<Image>);

#[derive(Default)]
struct M1Held(bool);

fn resample(total_length: f32, candidate_points: &Vec<Vec2>) -> Vec<Vec2> {
    const TARGET_POINTS: usize = 128;

    let mut resampled_points = Vec::with_capacity(TARGET_POINTS);
    resampled_points.push(candidate_points[0]);

    if candidate_points.len() > 1 {
        /*
         distance squared would be faster but using it leads to inaccuracies with the lerping and alpha;
         sqrting the alpha gives lesser points for some reason;
        */

        let increment = total_length / (TARGET_POINTS) as f32;
        let mut accumulated_distance = 0.0;
        let mut previous_point = candidate_points[0];

        for i in 1..candidate_points.len() {
            let current_point = candidate_points[i];
            let mut segment_distance = previous_point.distance(current_point);

            while accumulated_distance + segment_distance >= increment
                && resampled_points.len() < TARGET_POINTS
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

fn get_centroid(resampled_points: &Vec<Vec2>) -> Vec2 {
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    for point in resampled_points.iter() {
        sum_x += point.x;
        sum_y += point.y;
    }   
    sum_x /= resampled_points.len() as f32;
    sum_y /= resampled_points.len() as f32;
    Vec2::new(sum_x, sum_y)
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
        .add_systems(Update, draw)
        // .insert_resource(M1Held(false))
        .run();
}

fn draw(
    drawingboard: Res<DrawingBoard>,
    mut images: ResMut<Assets<Image>>,

    window: Single<&Window>,

    mut previous_pos: Local<Vec2>,
    mut m1held: Local<M1Held>,
    mut candidate_points: Local<Vec<Vec2>>,
    mut total_length: Local<f32>,

    mouse_delta: Res<AccumulatedMouseMotion>,
    mut button_events: EventReader<MouseButtonInput>,
) {
    for button_event in button_events.read() {
        if button_event.button == MouseButton::Left {
            if m1held.0 == false && button_event.state.is_pressed() == true {
                // started drawing so clear stuff
                candidate_points.clear();
                *total_length = 0.0;
                *previous_pos = Vec2::ZERO;

                let board = images.get_mut(&drawingboard.0).expect("Board not found!!");
                reset_board(window.size(), board, true);
            } else if m1held.0 == true && button_event.state.is_pressed() == false {
                // stopped drawing
                let board = images.get_mut(&drawingboard.0).expect("Board not found!!");
                reset_board(window.size(), board, false);

                let resampled_points = resample(*total_length, &candidate_points);
                println!("Resampled points count: {}", resampled_points.len());
                
                let c = get_centroid(&resampled_points);
                let indicative_angle = ops::atan2(c.y - resampled_points[0].y, c.x - resampled_points[0].x) + PI;
                // rotation of a point about origin formula was x = x'cosx + y'sinx and for y you add pi/2
                for (i,p) in resampled_points.iter().enumerate() {
                    let x_ = p.x - c.x;
                    let y_ = p.y - c.y;
                    let x = x_*ops::cos(indicative_angle) + y_*ops::sin(indicative_angle) + c.x;
                    let y = y_*ops::cos(indicative_angle) - x_*ops::sin(indicative_angle) + c.y;
                
                    board
                        .set_color_at(x as u32, y as u32, if i == 0 { Color::linear_rgb(255.0, 0.0, 0.0)} else { DRAW_COLOR })
                        .unwrap_or(());
                }
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
                            let x_e = vec.x + (delta_r as f32) * ops::cos((theta as f32).to_radians());
                            let y_e = vec.y + (delta_r as f32) * ops::sin((theta as f32).to_radians());
                            board
                                .set_color_at(x_e as u32, y_e as u32, DRAW_COLOR)
                                .unwrap_or(()); // most likely the error would be an out_of_bounds so it i think im okay to ignore
                        }
                    }
                } else {
                    board
                        .set_color_at(vec.x as u32, vec.y as u32, DRAW_COLOR)
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
