//! Hero background: an animated constellation of particles drawn on a canvas.
//! Pauses when the hero is off-screen (and the browser pauses rAF in hidden tabs).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gloo::events::EventListener;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, PointerEvent};
use yew::prelude::*;

use crate::dom::{self, RafLoop};

struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    r: f64,
    hue: bool,
}

#[derive(Default)]
struct Field {
    particles: Vec<Particle>,
    width: f64,
    height: f64,
    dpr: f64,
    mouse: Option<(f64, f64)>,
    frame: u32,
    light: bool,
}

fn rand() -> f64 {
    js_sys::Math::random()
}

impl Field {
    fn resize(&mut self, canvas: &HtmlCanvasElement) -> bool {
        let w = f64::from(canvas.client_width());
        let h = f64::from(canvas.client_height());
        if w <= 0.0 || h <= 0.0 {
            return false;
        }
        if (w - self.width).abs() > 0.5 || (h - self.height).abs() > 0.5 {
            let dpr = dom::window().device_pixel_ratio().clamp(1.0, 2.0);
            self.width = w;
            self.height = h;
            self.dpr = dpr;
            canvas.set_width((w * dpr) as u32);
            canvas.set_height((h * dpr) as u32);
            let target = ((w * h) / 13_000.0).clamp(26.0, 95.0) as usize;
            self.particles.truncate(target);
            while self.particles.len() < target {
                self.particles.push(Particle {
                    x: rand() * w,
                    y: rand() * h,
                    vx: (rand() - 0.5) * 0.45,
                    vy: (rand() - 0.5) * 0.45,
                    r: 0.8 + rand() * 1.6,
                    hue: rand() > 0.5,
                });
            }
            for p in &mut self.particles {
                p.x = p.x.min(w);
                p.y = p.y.min(h);
            }
        }
        true
    }

    fn step(&mut self, ctx: &CanvasRenderingContext2d, animate: bool) {
        if self.frame.is_multiple_of(30) {
            self.light = dom::document()
                .document_element()
                .and_then(|e| e.get_attribute("data-theme"))
                .is_some_and(|t| t == "light");
        }
        self.frame = self.frame.wrapping_add(1);
        let (w, h) = (self.width, self.height);
        let _ = ctx.set_transform(self.dpr, 0.0, 0.0, self.dpr, 0.0, 0.0);
        ctx.clear_rect(0.0, 0.0, w, h);

        if animate {
            for p in &mut self.particles {
                if let Some((mx, my)) = self.mouse {
                    let (dx, dy) = (p.x - mx, p.y - my);
                    let d2 = dx * dx + dy * dy;
                    if d2 < 120.0 * 120.0 && d2 > 1.0 {
                        let f = 0.6 / d2.sqrt();
                        p.vx += dx * f * 0.05;
                        p.vy += dy * f * 0.05;
                    }
                }
                p.vx = p.vx.clamp(-0.9, 0.9) * 0.995;
                p.vy = p.vy.clamp(-0.9, 0.9) * 0.995;
                if p.vx.abs() < 0.05 {
                    p.vx += (rand() - 0.5) * 0.02;
                }
                if p.vy.abs() < 0.05 {
                    p.vy += (rand() - 0.5) * 0.02;
                }
                p.x += p.vx;
                p.y += p.vy;
                if p.x < 0.0 || p.x > w {
                    p.vx = -p.vx;
                    p.x = p.x.clamp(0.0, w);
                }
                if p.y < 0.0 || p.y > h {
                    p.vy = -p.vy;
                    p.y = p.y.clamp(0.0, h);
                }
            }
        }

        let (c1, c2, link) = if self.light {
            ("92,64,230", "8,145,178", "92,64,230")
        } else {
            ("124,92,255", "34,211,238", "140,120,255")
        };
        // Links are batched into a few opacity buckets: one path + stroke per bucket.
        const BUCKETS: usize = 8;
        let max_d = 130.0;
        let mut buckets: [Vec<(f64, f64, f64, f64)>; BUCKETS] = Default::default();
        let n = self.particles.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let (a, b) = (&self.particles[i], &self.particles[j]);
                let (dx, dy) = (a.x - b.x, a.y - b.y);
                let d2 = dx * dx + dy * dy;
                if d2 < max_d * max_d {
                    let k = ((1.0 - d2.sqrt() / max_d) * BUCKETS as f64) as usize;
                    buckets[k.min(BUCKETS - 1)].push((a.x, a.y, b.x, b.y));
                }
            }
        }
        ctx.set_line_width(1.0);
        for (k, segs) in buckets.iter().enumerate() {
            if segs.is_empty() {
                continue;
            }
            let alpha = (k as f64 + 0.5) / BUCKETS as f64 * 0.38;
            ctx.set_stroke_style_str(&format!("rgba({link},{alpha:.3})"));
            ctx.begin_path();
            for &(x1, y1, x2, y2) in segs {
                ctx.move_to(x1, y1);
                ctx.line_to(x2, y2);
            }
            ctx.stroke();
        }
        if let Some((mx, my)) = self.mouse {
            // Warm links from the cursor to nearby particles.
            ctx.set_stroke_style_str("rgba(255,150,60,0.28)");
            ctx.begin_path();
            for p in &self.particles {
                let (dx, dy) = (p.x - mx, p.y - my);
                if dx * dx + dy * dy < 170.0 * 170.0 {
                    ctx.move_to(p.x, p.y);
                    ctx.line_to(mx, my);
                }
            }
            ctx.stroke();
        }
        for (hue, color) in [(true, c1), (false, c2)] {
            ctx.set_fill_style_str(&format!("rgba({color},0.9)"));
            ctx.begin_path();
            for p in self.particles.iter().filter(|p| p.hue == hue) {
                ctx.move_to(p.x + p.r, p.y);
                let _ = ctx.arc(p.x, p.y, p.r, 0.0, std::f64::consts::TAU);
            }
            ctx.fill();
        }
    }
}

/// Everything that must stay alive while the canvas is mounted.
type Running = (Rc<RefCell<Option<RafLoop>>>, Option<dom::IoHandle>, EventListener);

#[component]
pub fn Particles() -> Html {
    let canvas_ref = use_node_ref();
    {
        let canvas_ref = canvas_ref.clone();
        use_effect_with((), move |_| {
            let mut keep: Option<Running> = None;
            if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                let ctx = canvas
                    .get_context("2d")
                    .ok()
                    .flatten()
                    .and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok());
                if let Some(ctx) = ctx {
                    let field = Rc::new(RefCell::new(Field::default()));
                    let animate = !dom::prefers_reduced_motion();
                    let raf: Rc<RefCell<Option<RafLoop>>> = Rc::default();

                    let start = {
                        let (field, canvas, ctx, raf) = (field.clone(), canvas.clone(), ctx.clone(), raf.clone());
                        Rc::new(move || {
                            if raf.borrow().is_some() {
                                return;
                            }
                            let (field, canvas, ctx) = (field.clone(), canvas.clone(), ctx.clone());
                            let drawn = Cell::new(0u32);
                            *raf.borrow_mut() = Some(RafLoop::start(move |_| {
                                let mut f = field.borrow_mut();
                                if f.resize(&canvas) {
                                    f.step(&ctx, animate);
                                    drawn.set(drawn.get() + 1);
                                }
                                // Reduced motion: draw a static constellation and stop.
                                animate || drawn.get() < 2
                            }));
                        })
                    };

                    let io = {
                        let (start, raf) = (start.clone(), raf.clone());
                        dom::observe_visibility(&canvas, 0.0, move |visible| {
                            if visible {
                                start();
                            } else {
                                raf.borrow_mut().take();
                            }
                        })
                    };
                    if io.is_none() {
                        start();
                    }

                    let pointer = {
                        let (field, canvas) = (field.clone(), canvas.clone());
                        EventListener::new(&dom::window(), "pointermove", move |e| {
                            let Some(e) = e.dyn_ref::<PointerEvent>() else { return };
                            let r = canvas.get_bounding_client_rect();
                            let (x, y) = (f64::from(e.client_x()) - r.left(), f64::from(e.client_y()) - r.top());
                            let inside = x >= 0.0 && y >= 0.0 && x <= r.width() && y <= r.height();
                            field.borrow_mut().mouse = inside.then_some((x, y));
                        })
                    };
                    keep = Some((raf, io, pointer));
                }
            }
            move || {
                if let Some((raf, io, pointer)) = keep {
                    raf.borrow_mut().take();
                    drop(io);
                    drop(pointer);
                }
            }
        });
    }
    html! { <canvas ref={canvas_ref} class="particles" aria-hidden="true"></canvas> }
}
