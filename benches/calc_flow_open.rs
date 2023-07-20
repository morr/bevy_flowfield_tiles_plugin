//! Measure a FlowField generation for a world of uniform CostFields (hence open - open space)
//!
//! World is 100 sectors by 100 sectors
//!

use std::time::Duration;

use bevy::prelude::*;
// use bevy::{
// 	app::{AppExit, ScheduleRunnerPlugin},
// 	prelude::*,
// 	render::{settings::WgpuSettings, RenderPlugin}, winit::WinitSettings,
// };
use bevy_flowfield_tiles_plugin::prelude::*;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// fn setup(mut cmds: Commands) {
// 	cmds.spawn(FlowFieldTilesBundle::new(1000, 1000));
// }

// fn request(mut event: EventWriter<EventPathRequest>) {
// 	// top right
// 	let source_sector = (99, 0);
// 	let source_field_cell = (9, 0);
// 	// bottom left
// 	let target_sector = (0, 99);
// 	let target_goal = (0, 9);

// 	event.send(EventPathRequest::new(
// 		source_sector,
// 		source_field_cell,
// 		target_sector,
// 		target_goal,
// 	))
// }

// fn check_for_flowfield(
// 	flow_cache_q: Query<&FlowFieldCache>,
// 	mut app_exit_events: EventWriter<AppExit>,
// ) {
// 	let flow_cache = flow_cache_q.get_single().unwrap();
// 	if !flow_cache.get().is_empty() {
// 		app_exit_events.send(AppExit);
// 	}
// }

/// Create the required CostFields and Portals before benchmarking
fn prepare_fields(
	map_length: u32,
	map_depth: u32,
) -> (SectorPortals, SectorCostFields, MapDimensions, RouteCache) {
	let map_dimensions = MapDimensions::new(map_length, map_depth);
	let cost_fields = SectorCostFields::new(map_length, map_depth);
	let mut portals = SectorPortals::new(map_dimensions.get_column(), map_dimensions.get_row());
	// update default portals for cost fields
	for sector_id in cost_fields.get().keys() {
		portals.update_portals(
			*sector_id,
			&cost_fields,
			map_dimensions.get_column(),
			map_dimensions.get_row(),
		);
	}
	let graph = PortalGraph::new(
		&portals,
		&cost_fields,
		map_dimensions.get_column(),
		map_dimensions.get_row(),
	);

	let mut route_cache = RouteCache::default();
	// top right
	let source_sector = (99, 0);
	let source_field_cell = (9, 0);
	let source = (source_sector, source_field_cell);
	// bottom left
	let target_sector = (0, 99);
	let target_goal = (0, 9);
	let target = (target_sector, target_goal);

	// find the route
	let node_route = graph
		.find_best_path(source, target, &portals, &cost_fields)
		.unwrap();
	let mut path = graph.convert_index_path_to_sector_portal_cells(node_route.1, &portals);
	filter_path(&mut path, target_goal);
	route_cache.insert_route(
		source_sector,
		target_sector,
		target_goal,
		Duration::default(),
		path,
	);

	(portals, cost_fields, map_dimensions, route_cache)
}

/// Create the components of a FlowFieldTilesBundle and drive them with an actor in the top right
/// corner pathing to the bottom left
fn flow_open(
	portals: SectorPortals,
	cost_fields: SectorCostFields,
	map_dimensions: MapDimensions,
	route_cache: RouteCache,
) {
	let mut flow_cache = FlowFieldCache::default();
	// generate flow
	for (_key, portal_path) in route_cache.get().iter() {
		// original order is from actor to goal, int fields need to be processed the other way around
		let mut path = portal_path.clone();
		path.reverse();
		let mut sectors_expanded_goals = Vec::new();
		for (i, (sector_id, goal)) in path.iter().enumerate() {
			// // only run if a FlowField hasn't been generated
			// if !field_cache.get().contains_key(&(*sector_id, *goal)) {
			// first element is always the end target, don't bother with portal expansion
			if i == 0 {
				sectors_expanded_goals.push((*sector_id, vec![*goal]));
			} else {
				// portals represent the boundary to another sector, a portal can be spread over
				// multple grid cells, expand the portal to provide multiple goal
				// targets for moving to another sector
				let neighbour_sector_id = path[i - 1].0;
				let g = portals
					.get()
					.get(sector_id)
					.unwrap()
					.expand_portal_into_goals(
						&cost_fields,
						sector_id,
						goal,
						&neighbour_sector_id,
						map_dimensions.get_column(),
						map_dimensions.get_row(),
					);
				sectors_expanded_goals.push((*sector_id, g));
			}
			// }
		}
		// build the integration fields
		let mut sector_int_fields = Vec::new();
		for (sector_id, goals) in sectors_expanded_goals.iter() {
			let mut int_field = IntegrationField::new(goals);
			let cost_field = cost_fields.get().get(sector_id).unwrap();
			int_field.calculate_field(goals, cost_field);
			sector_int_fields.push((*sector_id, goals.clone(), int_field));
		}
		// build the flow fields
		for (i, (sector_id, goals, int_field)) in sector_int_fields.iter().enumerate() {
			let mut flow_field = FlowField::default();
			// first element is end target, therefore has no info about previous sector for
			// direction optimisations
			if i == 0 {
				flow_field.calculate(goals, None, int_field);
				flow_cache.insert_field(*sector_id, path[i].1, Duration::default(), flow_field);
			} else if let Some(dir_prev_sector) =
				Ordinal::sector_to_sector_direction(sector_int_fields[i - 1].0, *sector_id)
			{
				let prev_int_field = &sector_int_fields[i - 1].2;
				flow_field.calculate(goals, Some((dir_prev_sector, prev_int_field)), int_field);
				//TODO by using the portal goal from path[i].1 actors criss-crossing from two seperate routes means one will use the others route in a sector which may be less efficient then using thier own
				flow_cache.insert_field(*sector_id, path[i].1, Duration::default(), flow_field);
			} else {
				error!("Route {:?}", portal_path);
			};
		}
	}
}

pub fn criterion_benchmark(c: &mut Criterion) {
	let mut group = c.benchmark_group("algorithm_use");
	group.significance_level(0.05).sample_size(100);
	let (portals, cost_fields, map_dimensions, route_cache) = prepare_fields(1000, 1000);
	group.bench_function("calc_flow_open", |b| {
		b.iter(|| {
			flow_open(
				black_box(portals.clone()),
				black_box(cost_fields.clone()),
				black_box(map_dimensions.clone()),
				black_box(route_cache.clone()),
			)
		})
	});
	group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
