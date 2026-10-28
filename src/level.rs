use macroquad::prelude::*;

pub const TILE: f32 = 32.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    Empty,
    Solid,
    OneWay,
    Spike,
    /// Solid jump pad: landing on it launches you.
    Bounce,
    /// Solid until stood on, then dissolves for a while.
    Crumble,
    /// Deadly liquid.
    Acid,
    /// Solid until the player has collected enough coins.
    Gate,
    /// A ramp: solid below a straight line across the tile, open above it.
    Slope(Slope),
    /// Solid in one half of the cell.
    Half(Half),
}

/// Which half of a half block is solid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Half {
    Lower,
    Left,
    Upper,
    Right,
}

impl Half {
    /// Level characters, in rotation order (each a quarter turn clockwise from the last).
    pub const ALL: [(char, Half); 4] = [('_', Half::Lower), ('[', Half::Left), ('T', Half::Upper), (']', Half::Right)];

    pub fn from_char(c: char) -> Option<Half> {
        Self::ALL.iter().find(|(ch, _)| *ch == c).map(|(_, h)| *h)
    }

    /// The solid part of a cell whose top-left corner is at (x, y).
    pub fn rect(self, x: f32, y: f32) -> Rect {
        let h = TILE / 2.0;
        match self {
            Half::Lower => Rect::new(x, y + h, TILE, h),
            Half::Upper => Rect::new(x, y, TILE, h),
            Half::Left => Rect::new(x, y, h, TILE),
            Half::Right => Rect::new(x + h, y, h, TILE),
        }
    }
}

/// A ramp's floor height at its left and right edges, in half tiles (0..=2).
/// `/` is (0, 2), `\` is (2, 0); gentle ramps take two tiles: `12` rising, `34` falling.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slope {
    pub left: u8,
    pub right: u8,
}

impl Slope {
    pub const ALL: [(char, Slope); 6] = [
        ('/', Slope { left: 0, right: 2 }),
        ('\\', Slope { left: 2, right: 0 }),
        ('1', Slope { left: 0, right: 1 }),
        ('2', Slope { left: 1, right: 2 }),
        ('3', Slope { left: 2, right: 1 }),
        ('4', Slope { left: 1, right: 0 }),
    ];

    pub fn from_char(c: char) -> Option<Slope> {
        Self::ALL.iter().find(|(ch, _)| *ch == c).map(|(_, s)| *s)
    }

    /// Floor height (px above the tile's bottom) at `fx` (0..1) across the tile.
    pub fn height_at(self, fx: f32) -> f32 {
        let (l, r) = (self.left as f32, self.right as f32);
        (l + (r - l) * fx.clamp(0.0, 1.0)) * TILE / 2.0
    }

    /// dy/dx of the floor in screen space (y down): negative when it rises to the right.
    pub fn gradient(self) -> f32 {
        -(self.right as f32 - self.left as f32) / 2.0
    }
}

/// A floor found under the player: its surface height and gradient (dy/dx, y down).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Floor {
    pub y: f32,
    pub gradient: f32,
}

/// Purely visual objects on the decoration layer. They never collide.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecorKind {
    /// Potted plant standing on the bottom of the cell.
    Cilia,
    /// Vines hanging from the top of the cell.
    Tendril,
    /// Security camera that tracks the player.
    Eye,
    /// Rooftop AC unit.
    Cysts,
    /// Pipe across the cell.
    Scar,
    /// Antenna mast.
    Veins,
    /// Shows its text when the player is near.
    Sign,
    /// Advert screen; neighbouring screens join into one big screen.
    Screen,
    /// Neon sign showing typed text.
    Neon,
    /// Grass turf over the floor below.
    Grass,
    /// Handrail along a roof edge.
    Railing,
    /// Red maintenance ladder.
    Ladder,
    /// Floodlight on a pole.
    Floodlight,
    /// Glass skylight on a roof.
    Skylight,
    /// Stacked crates.
    Crates,
}

impl DecorKind {
    pub fn from_char(c: char) -> Option<Self> {
        Some(match c {
            'c' => DecorKind::Cilia,
            't' => DecorKind::Tendril,
            'e' => DecorKind::Eye,
            'u' => DecorKind::Cysts,
            'x' => DecorKind::Scar,
            'v' => DecorKind::Veins,
            'S' => DecorKind::Sign,
            'A' => DecorKind::Screen,
            'N' => DecorKind::Neon,
            'g' => DecorKind::Grass,
            'r' => DecorKind::Railing,
            'l' => DecorKind::Ladder,
            'f' => DecorKind::Floodlight,
            'k' => DecorKind::Skylight,
            'b' => DecorKind::Crates,
            _ => return None,
        })
    }
}

/// Walls standing at half the building depth, on their own layer so decorations
/// can go in front of them. The same kinds make up the front layer, which stands
/// nearer than the player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallKind {
    Wall,
    /// Wall with glass in it; neighbouring windows join into one pane.
    Window,
    /// Bottom half of the cell only.
    Low,
    /// A column, half a cell wide.
    Pillar,
    /// Full-height glazing.
    Glass,
    /// Roller shutter.
    Shutter,
    /// Painted in an accent colour.
    Accent,
}

impl WallKind {
    pub fn from_char(c: char) -> Option<Self> {
        Some(match c {
            'w' => WallKind::Wall,
            'n' => WallKind::Window,
            'h' => WallKind::Low,
            'p' => WallKind::Pillar,
            'g' => WallKind::Glass,
            'd' => WallKind::Shutter,
            'a' => WallKind::Accent,
            _ => return None,
        })
    }
}

pub struct WallCell {
    pub kind: WallKind,
    pub x: i32,
    pub y: i32,
}

pub struct Decor {
    pub kind: DecorKind,
    pub x: i32,
    pub y: i32,
    /// Quarter turns clockwise (pipes and cameras).
    pub rot: u8,
}

pub struct Sign {
    pub x: i32,
    pub y: i32,
    pub text: String,
}

pub struct GateCell {
    pub x: i32,
    pub y: i32,
    /// Coins needed to open it.
    pub need: u32,
}

/// `\n` in a level file is a line break in sign text.
pub fn decode_sign_text(s: &str) -> String {
    s.replace("\\n", "\n")
}

pub fn encode_sign_text(s: &str) -> String {
    s.replace('\n', "\\n")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnemyKind {
    /// Walks along the ground, turning at walls and ledges.
    Crawler,
    /// Hovers in place.
    Floater,
}

/// Tile map parsed from an ASCII file.
///
/// Legend:
///   `#` solid   `%` slick block (solid, but no wall-runs, wall-jumps or mantles)
///   `=` one-way platform   `B` bounce block   `X` crumble block
///   `^` spikes  `~` acid   `E` crawler enemy   `F` floater enemy
///   `o` coin    `D` dash orb   `J` jump bubble   `L` gate
///   `P` player spawn   `C` checkpoint   `G` goal   anything else = empty
///   ramps: `/` and `\` (steep), `12` (gentle, rising) and `34` (gentle, falling)
///   half blocks: `_` lower `T` upper `[` left `]` right
///
/// Lines starting with `@` are metadata:
///   `@theme Ichor`
///   `@gate X Y N`     the gate cell at X,Y needs N coins (default: all of them)
///   `@sign X Y text`  text for the sign at X,Y (`\n` for a line break)
///   `@sign X Y text`  is also the text of a neon sign (`N`) at X,Y
///   `@rot X Y N`      the decoration at X,Y is turned N quarter turns clockwise
///   `@decor`          the rows after this line are the decoration layer:
///                     `c` plant `t` vines `e` camera `u` AC unit `x` pipe `v` antenna `S` sign
///                     `A` screen `N` neon `g` grass `r` railing `l` ladder `f` floodlight
///                     `k` skylight `b` crates
///   `@walls`          the rows after this line are the wall layer (half depth):
///                     `w` wall `n` window `h` low wall `p` pillar `g` glass `d` shutter `a` accent
///   `@front`          the rows after this line are the front wall layer (same kinds as
///                     `@walls`), standing between the player and the camera
pub struct Level {
    pub width: i32,
    pub height: i32,
    tiles: Vec<Tile>,
    /// Solid cells too smooth to grip: no wall-runs, wall-jumps or mantles.
    slick: Vec<bool>,
    /// Crumble blocks that are dissolved and gates that are open (not solid).
    gone: Vec<bool>,
    /// Feet position of the spawn point.
    pub spawn: Vec2,
    /// Coin centers.
    pub coins: Vec<Vec2>,
    /// Dash orb centers.
    pub dash_orbs: Vec<Vec2>,
    /// Jump bubble centers.
    pub bubbles: Vec<Vec2>,
    /// Enemy kind and the cell centre it starts in.
    pub enemies: Vec<(EnemyKind, Vec2)>,
    /// Feet positions of checkpoint flags.
    pub checkpoints: Vec<Vec2>,
    /// Feet position of the goal flag.
    pub goal: Vec2,
    /// Index into `palette::THEMES`.
    pub theme: usize,
    pub gates: Vec<GateCell>,
    pub decor: Vec<Decor>,
    pub signs: Vec<Sign>,
    /// Neon signs and their text.
    pub neons: Vec<Sign>,
    pub walls: Vec<WallCell>,
    /// Walls in front of the player.
    pub front_walls: Vec<WallCell>,
}

impl Level {
    pub fn parse(src: &str) -> Self {
        let mut theme = 0;
        let mut gate_need = std::collections::HashMap::new();
        let mut sign_text = std::collections::HashMap::new();
        let mut rotation = std::collections::HashMap::new();
        let mut rows: Vec<&str> = Vec::new();
        let mut decor_rows: Vec<&str> = Vec::new();
        let mut wall_rows: Vec<&str> = Vec::new();
        let mut front_rows: Vec<&str> = Vec::new();
        // 0 = tiles, 1 = decor, 2 = walls, 3 = front walls.
        let mut section = 0;
        for line in src.lines().map(|l| l.trim_end()).filter(|l| !l.is_empty()) {
            let Some(meta) = line.trim_start().strip_prefix('@') else {
                match section {
                    1 => decor_rows.push(line),
                    2 => wall_rows.push(line),
                    3 => front_rows.push(line),
                    _ => rows.push(line),
                }
                continue;
            };
            let mut parts = meta.splitn(4, ' ');
            let cmd = parts.next().unwrap_or("");
            let mut xy = || -> Option<(i32, i32)> { Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?)) };
            match cmd {
                "theme" => theme = crate::palette::theme_index(&meta["theme".len()..]).unwrap_or(0),
                "decor" => section = 1,
                "walls" => section = 2,
                "front" => section = 3,
                "gate" => {
                    let pos = xy();
                    if let (Some(pos), Some(n)) = (pos, parts.next().and_then(|n| n.trim().parse::<u32>().ok())) {
                        gate_need.insert(pos, n);
                    }
                }
                "rot" => {
                    if let (Some(pos), Some(n)) = (xy(), parts.next().and_then(|n| n.trim().parse::<u8>().ok())) {
                        rotation.insert(pos, n % 4);
                    }
                }
                "sign" => {
                    if let Some(pos) = xy() {
                        sign_text.insert(pos, decode_sign_text(parts.next().unwrap_or("").trim()));
                    }
                }
                _ => {}
            }
        }
        let height = rows.len() as i32;
        let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as i32;

        let mut tiles = vec![Tile::Empty; (width * height) as usize];
        let mut slick = vec![false; tiles.len()];
        let mut spawn = vec2(TILE * 2.0, TILE * 2.0);
        let mut coins = Vec::new();
        let mut checkpoints = Vec::new();
        let mut dash_orbs = Vec::new();
        let mut bubbles = Vec::new();
        let mut enemies = Vec::new();
        let mut goal = None;
        let mut gate_cells = Vec::new();

        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                let idx = y * width as usize + x;
                let center = vec2(x as f32 * TILE + TILE * 0.5, y as f32 * TILE + TILE * 0.5);
                let feet = vec2(center.x, (y as f32 + 1.0) * TILE);
                match ch {
                    '#' => tiles[idx] = Tile::Solid,
                    '%' => {
                        tiles[idx] = Tile::Solid;
                        slick[idx] = true;
                    }
                    '=' => tiles[idx] = Tile::OneWay,
                    '^' => tiles[idx] = Tile::Spike,
                    'B' => tiles[idx] = Tile::Bounce,
                    'X' => tiles[idx] = Tile::Crumble,
                    '~' => tiles[idx] = Tile::Acid,
                    'E' => enemies.push((EnemyKind::Crawler, center)),
                    'F' => enemies.push((EnemyKind::Floater, center)),
                    'o' => coins.push(center),
                    'D' => dash_orbs.push(center),
                    'J' => bubbles.push(center),
                    'P' => spawn = feet,
                    'C' => checkpoints.push(feet),
                    'G' => goal = Some(feet),
                    c if Slope::from_char(c).is_some() => tiles[idx] = Tile::Slope(Slope::from_char(c).unwrap()),
                    c if Half::from_char(c).is_some() => tiles[idx] = Tile::Half(Half::from_char(c).unwrap()),
                    'L' => {
                        tiles[idx] = Tile::Gate;
                        gate_cells.push((x as i32, y as i32));
                    }
                    _ => {}
                }
            }
        }

        let total = coins.len() as u32;
        let gates = gate_cells
            .into_iter()
            .map(|(x, y)| GateCell { x, y, need: gate_need.get(&(x, y)).copied().unwrap_or(total) })
            .collect();
        let mut decor = Vec::new();
        let mut signs = Vec::new();
        let mut neons = Vec::new();
        let mut walls = Vec::new();
        for (y, row) in decor_rows.iter().enumerate().take(height as usize) {
            for (x, ch) in row.chars().enumerate().take(width as usize) {
                let (x, y) = (x as i32, y as i32);
                // Older levels kept walls and windows on the decor layer.
                if matches!(ch, 'w' | 'n') {
                    walls.push(WallCell { kind: WallKind::from_char(ch).unwrap(), x, y });
                    continue;
                }
                let Some(kind) = DecorKind::from_char(ch) else { continue };
                let text = || sign_text.get(&(x, y)).cloned().unwrap_or_default();
                match kind {
                    DecorKind::Sign => signs.push(Sign { x, y, text: text() }),
                    DecorKind::Neon => neons.push(Sign { x, y, text: text() }),
                    _ => {}
                }
                decor.push(Decor { kind, x, y, rot: rotation.get(&(x, y)).copied().unwrap_or(0) });
            }
        }
        let wall_layer = |rows: &[&str]| {
            let mut out = Vec::new();
            for (y, row) in rows.iter().enumerate().take(height as usize) {
                for (x, ch) in row.chars().enumerate().take(width as usize) {
                    if let Some(kind) = WallKind::from_char(ch) {
                        out.push(WallCell { kind, x: x as i32, y: y as i32 });
                    }
                }
            }
            out
        };
        walls.extend(wall_layer(&wall_rows));
        let front_walls = wall_layer(&front_rows);

        Level {
            width,
            height,
            gone: vec![false; tiles.len()],
            tiles,
            slick,
            spawn,
            coins,
            dash_orbs,
            bubbles,
            enemies,
            checkpoints,
            goal: goal.unwrap_or(vec2(width as f32 * TILE - TILE, height as f32 * TILE)),
            theme,
            gates,
            decor,
            signs,
            neons,
            walls,
            front_walls,
        }
    }

    /// Out of bounds to the left/right counts as solid wall; above/below is open.
    pub fn tile(&self, x: i32, y: i32) -> Tile {
        if x < 0 || x >= self.width {
            return Tile::Solid;
        }
        if y < 0 || y >= self.height {
            return Tile::Empty;
        }
        self.tiles[(y * self.width + x) as usize]
    }

    /// Whether a tile blocks movement right now (crumble blocks can be dissolved).
    pub fn is_solid(&self, x: i32, y: i32) -> bool {
        match self.tile(x, y) {
            Tile::Solid | Tile::Bounce => true,
            Tile::Crumble | Tile::Gate => !self.is_gone(x, y),
            _ => false,
        }
    }

    /// Whether a cell is a slick block (nothing to grip on its sides).
    pub fn is_slick(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height && self.slick[(y * self.width + x) as usize]
    }

    pub fn is_gone(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height && self.gone[(y * self.width + x) as usize]
    }

    pub fn set_gone(&mut self, x: i32, y: i32, gone: bool) {
        if x >= 0 && y >= 0 && x < self.width && y < self.height {
            self.gone[(y * self.width + x) as usize] = gone;
        }
    }

    pub fn pixel_width(&self) -> f32 {
        self.width as f32 * TILE
    }

    pub fn pixel_height(&self) -> f32 {
        self.height as f32 * TILE
    }

    /// The solid area of a cell right now (world space), if any. Half blocks fill half of it.
    pub fn solid_rect(&self, x: i32, y: i32) -> Option<Rect> {
        let (wx, wy) = (x as f32 * TILE, y as f32 * TILE);
        match self.tile(x, y) {
            Tile::Half(h) => Some(h.rect(wx, wy)),
            _ if self.is_solid(x, y) => Some(Rect::new(wx, wy, TILE, TILE)),
            _ => None,
        }
    }

    /// Whether a world point is inside something solid.
    pub fn point_solid(&self, x: f32, y: f32) -> bool {
        self.solid_rect((x / TILE).floor() as i32, (y / TILE).floor() as i32)
            .is_some_and(|s| x >= s.x && x < s.x + s.w && y >= s.y && y < s.y + s.h)
    }

    /// Solid areas overlapping `r` (touching edges don't count).
    fn solids_in(&self, r: Rect) -> impl Iterator<Item = Rect> + '_ {
        let (x0, x1, y0, y1) = tile_span(r);
        (y0..=y1)
            .flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
            .filter_map(|(x, y)| self.solid_rect(x, y))
            .filter(move |s| r.x < s.x + s.w && r.x + r.w > s.x && r.y < s.y + s.h && r.y + r.h > s.y)
    }

    pub fn rect_hits_solid(&self, r: Rect) -> bool {
        self.solids_in(r).next().is_some()
    }

    /// The lowest underside among the solids overlapping `r` (for bumping your head).
    pub fn solid_bottom_in(&self, r: Rect) -> Option<f32> {
        self.solids_in(r).map(|s| s.y + s.h).reduce(f32::max)
    }

    /// Top of the solid stack containing the point (x, y), climbing up through
    /// anything solid directly above it. None if the point isn't solid.
    pub fn solid_top_at(&self, x: f32, y: f32) -> Option<f32> {
        if !self.point_solid(x, y) {
            return None;
        }
        let mut y = y;
        for _ in 0..(self.height * 2 + 2) {
            let (cx, cy) = ((x / TILE).floor() as i32, (y / TILE).floor() as i32);
            let s = self.solid_rect(cx, cy)?;
            if s.y <= 0.0 || !self.point_solid(x, s.y - 0.01) {
                return Some(s.y);
            }
            y = s.y - 0.01;
        }
        None
    }

    pub fn slope(&self, x: i32, y: i32) -> Option<Slope> {
        match self.tile(x, y) {
            Tile::Slope(s) => Some(s),
            _ => None,
        }
    }

    /// The highest floor surface in the column at world `x` with `min_y <= y <= max_y`.
    /// Solid tops and ramp surfaces count; one-way platforms count only if their top is
    /// at or below `one_way_min_y` (so you can't snap onto one from underneath).
    pub fn floor_at(&self, x: f32, min_y: f32, max_y: f32, one_way_min_y: f32) -> Option<Floor> {
        if max_y < min_y {
            return None;
        }
        let tx = (x / TILE).floor() as i32;
        let fx = x / TILE - tx as f32;
        let ty0 = (min_y / TILE).floor() as i32;
        let ty1 = (max_y / TILE).floor() as i32;
        for ty in ty0..=ty1 {
            let top = ty as f32 * TILE;
            let surface = match self.tile(tx, ty) {
                Tile::Slope(s) => Some(Floor { y: top + TILE - s.height_at(fx), gradient: s.gradient() }),
                Tile::OneWay if top >= one_way_min_y => Some(Floor { y: top, gradient: 0.0 }),
                // The top of a solid area under this x, if nothing sits on it.
                _ => self
                    .solid_rect(tx, ty)
                    .filter(|s| x >= s.x && x < s.x + s.w)
                    .filter(|s| !self.point_solid(x, s.y - 0.5) && (s.y != top || self.slope(tx, ty - 1).is_none()))
                    .map(|s| Floor { y: s.y, gradient: 0.0 }),
            };
            if let Some(f) = surface.filter(|f| f.y >= min_y && f.y <= max_y) {
                return Some(f);
            }
        }
        None
    }

    /// Spikes (forgiving hitbox) and acid (below its surface).
    pub fn rect_hits_hazard(&self, r: Rect) -> bool {
        let (x0, x1, y0, y1) = tile_span(r);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (tx, ty) = (x as f32 * TILE, y as f32 * TILE);
                let hb = match self.tile(x, y) {
                    // Only the lower, pointy part of a spike tile hurts.
                    Tile::Spike => Rect::new(tx + 5.0, ty + 12.0, TILE - 10.0, TILE - 12.0),
                    // You can skim the very top of an acid pool.
                    Tile::Acid => {
                        let dip = if self.tile(x, y - 1) == Tile::Acid { 0.0 } else { 8.0 };
                        Rect::new(tx, ty + dip, TILE, TILE - dip)
                    }
                    _ => continue,
                };
                if r.overlaps(&hb) {
                    return true;
                }
            }
        }
        false
    }

    /// Every cell of a given tile type.
    pub fn cells(&self, kind: Tile) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if self.tile(x, y) == kind {
                    out.push((x, y));
                }
            }
        }
        out
    }
}

/// Inclusive tile range (x0, x1, y0, y1) covered by a rect.
pub fn tile_span(r: Rect) -> (i32, i32, i32, i32) {
    (
        (r.x / TILE).floor() as i32,
        ((r.x + r.w - 0.01) / TILE).floor() as i32,
        (r.y / TILE).floor() as i32,
        ((r.y + r.h - 0.01) / TILE).floor() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_walls_and_neon_text() {
        let level = Level::parse("@sign 0 0 OPEN\n...\n###\n@decor\nNg.\n...\n@walls\nhpd\n...\n");
        let kinds: Vec<WallKind> = level.walls.iter().map(|w| w.kind).collect();
        assert_eq!(kinds, [WallKind::Low, WallKind::Pillar, WallKind::Shutter]);
        assert_eq!(level.neons[0].text, "OPEN");
        assert!(level.signs.is_empty(), "neon text isn't a sign popup");
        assert_eq!(level.decor[1].kind, DecorKind::Grass);
    }

    #[test]
    fn parses_slick_blocks_and_front_walls() {
        let level = Level::parse("...
#%#
@walls
w..
...
@front
.p.
...
");
        assert!(level.is_solid(1, 1) && level.is_slick(1, 1) && !level.is_slick(0, 1));
        assert_eq!(level.walls.len(), 1);
        assert_eq!(level.front_walls.len(), 1);
        assert_eq!((level.front_walls[0].kind, level.front_walls[0].x), (WallKind::Pillar, 1));
    }

    #[test]
    fn walls_on_the_old_decor_layer_still_load() {
        let level = Level::parse("...\n###\n@decor\nwnw\n...\n");
        let kinds: Vec<WallKind> = level.walls.iter().map(|w| w.kind).collect();
        assert_eq!(kinds, [WallKind::Wall, WallKind::Window, WallKind::Wall]);
        assert!(level.decor.is_empty());
    }

    #[test]
    fn parses_gates_decor_and_signs() {
        let src = "@theme Rot\n@gate 2 0 1\n@sign 0 0 Turn\\nback\n..L.o\n#####\n@decor\nS.t\nvvv\n";
        let l = Level::parse(src);
        assert_eq!(l.gates.len(), 1);
        assert_eq!((l.gates[0].x, l.gates[0].y, l.gates[0].need), (2, 0, 1));
        assert!(l.is_solid(2, 0));
        assert_eq!(l.decor.len(), 5);
        assert_eq!(l.signs.len(), 1);
        assert_eq!(l.signs[0].text, "Turn\nback");

        // Without a count, a gate needs every coin (none here, so it opens at once).
        let l = Level::parse("..L\n###\n");
        assert_eq!(l.gates[0].need, 0);
        assert_eq!(decode_sign_text(&encode_sign_text("a\nb")), "a\nb");
    }

    #[test]
    fn ramps_are_floors_along_their_line() {
        let l = Level::parse("..../\n...12#\n#####\n");
        assert_eq!(l.slope(4, 0), Some(Slope { left: 0, right: 2 }));
        // Steep ramp: halfway across it the floor is halfway up the tile.
        let f = l.floor_at(4.5 * TILE, 0.0, 2.0 * TILE, f32::INFINITY).unwrap();
        assert!((f.y - 0.5 * TILE).abs() < 0.01 && f.gradient == -1.0, "{f:?}");
        // Gentle ramp rises half a tile per tile.
        let f = l.floor_at(3.5 * TILE, 0.0, 3.0 * TILE, f32::INFINITY).unwrap();
        assert!((f.y - 1.75 * TILE).abs() < 0.01 && f.gradient == -0.5, "{f:?}");
        // Ramps never block sideways movement; only real blocks are solid.
        assert!(!l.is_solid(4, 0) && l.is_solid(5, 1));
        assert!(l.floor_at(5.5 * TILE, 0.0, 3.0 * TILE, f32::INFINITY).is_some_and(|f| f.y == TILE));
        // The block under a ramp isn't a floor of its own.
        let under = Level::parse("/\n#\n");
        assert!(under.floor_at(0.5 * TILE, TILE, 2.0 * TILE, f32::INFINITY).is_none());
    }
}
