use bevy_ecs::prelude::*;
use game_common::{
    app::{AppBuilder, Plugin},
    world::Cell,
    ServerPacket,
};
use tokio::sync::mpsc;
use tracing::{debug, warn};
#[derive(Debug)]
struct Cells {
    width: u32,
    height: u32,
    // double buffering
    cells_a: CellsInner,
    cells_b: CellsInner,
    active: Active,
}

// which buffer is active
#[derive(Debug, Copy, Clone)]
enum Active {
    A,
    B,
}

impl Active {
    fn swap(&self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

impl Cells {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cells_a: CellsInner::new(width, height),
            cells_b: CellsInner::new(width, height),
            active: Active::A,
        }
    }

    pub fn current(&self) -> &[Cell] {
        self.inner_back().cells()
    }

    fn inner_active(&self) -> &CellsInner {
        match self.active {
            Active::A => &self.cells_a,
            Active::B => &self.cells_b,
        }
    }

    fn inner_active_mut(&mut self) -> &mut CellsInner {
        match self.active {
            Active::A => &mut self.cells_a,
            Active::B => &mut self.cells_b,
        }
    }

    fn inner_back(&self) -> &CellsInner {
        match self.active {
            Active::A => &self.cells_b,
            Active::B => &self.cells_a,
        }
    }

    fn neighborhoods(&self) -> impl Iterator<Item = ((u32, u32), AB<Neighborhood>)> {
        (0..self.height)
            .flat_map(move |y| (0..self.width).map(move |x| (x, y)))
            .filter_map(move |(x, y)| self.neighborhood(x, y).map(|n| ((x, y), n)))
    }

    fn set_at(&mut self, x: u32, y: u32, cell: Cell) -> Option<()> {
        self.inner_active_mut().set_at(x, y, cell)
    }

    fn neighborhood(&self, center_x: u32, center_y: u32) -> Option<AB<Neighborhood>> {
        let a = self.cells_a.neighborhood(center_x, center_y)?;
        let b = self.cells_b.neighborhood(center_x, center_y)?;
        Some(AB { a, b })
    }

    pub fn swap(&mut self) {
        self.active = self.active.swap();
    }
}

// both cells_a and cells_b values of T
#[derive(Debug, Copy, Clone)]
struct AB<T> {
    a: T,
    b: T,
}

#[derive(Debug)]
struct CellsInner {
    width: u32,
    height: u32,
    cells: Vec<Cell>,
}

impl CellsInner {
    fn new(width: u32, height: u32) -> Self {
        let cells = vec![Cell::Empty; width as usize * height as usize];
        Self {
            width,
            height,
            cells,
        }
    }

    fn cells(&self) -> &[Cell] {
        &self.cells
    }

    fn set_at(&mut self, x: u32, y: u32, cell: Cell) -> Option<()> {
        let index = self.cell_index(x, y)?;
        self.cells[index] = cell;
        Some(())
    }

    fn neighborhood(&self, center_x: u32, center_y: u32) -> Option<Neighborhood> {
        let mut neighborhood = [Cell::Empty; 9];
        for (i, (relative_x, relative_y)) in NEIGHBORHOOD.iter().enumerate() {
            let x = ((center_x as i64) + relative_x) as u32;
            let y = ((center_y as i64) + relative_y) as u32;
            neighborhood[i] = self.cell_at(x, y)?;
        }
        Some(neighborhood)
    }

    fn cell_at(&self, x: u32, y: u32) -> Option<Cell> {
        let index = self.cell_index(x, y)?;
        self.cells.get(index).copied()
    }

    fn cell_index(&self, x: u32, y: u32) -> Option<usize> {
        let x_index = self.height.checked_mul(x)?;
        let y_index = y;
        let index = x_index.checked_add(y_index)?;
        Some(index as usize)
    }
}

// [nw, n, ne, w, c, e, sw, s, se]
type Neighborhood<'a> = [Cell; 9];

const NEIGHBORHOOD: [(i64, i64); 9] = [
    (-1, 1),
    (0, 1),
    (1, 1),
    (-1, 0),
    (0, 0),
    (1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&mut self, app: AppBuilder) -> AppBuilder {
        app.insert_resource(Cells::new(1024, 1024))
            .add_system(advance_cells.system())
            .add_system(send_state.system())
    }
}

#[derive(Debug, Clone)]
enum CellChange {
    Set { x: u32, y: u32, cell: Cell },
}

fn send_state(cells: Res<Cells>, broadcast: Res<mpsc::UnboundedSender<ServerPacket>>) {
    if let Err(_) = broadcast.send(ServerPacket::UpdateCells { cells: vec![] }) {
        warn!("failed to send");
    }
}

fn advance_cells(mut cells: ResMut<Cells>) {
    let changes = cells
        .neighborhoods()
        .map(|(position, _neighborhood)| CellChange::Set {
            x: position.0,
            y: position.1,
            cell: Cell::Stone,
        })
        .collect::<Vec<_>>();
    for change in changes {
        match change {
            CellChange::Set { x, y, cell } => {
                cells.set_at(x, y, cell);
            }
        }
    }
    cells.swap();
}
