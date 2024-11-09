use geo::{Contains, LineString, Point, Polygon};
use serde::Serialize;
use std::sync::LazyLock;
use thousands::Separable;
use vatsim_utils::models::Pilot;
use vzdv::config::Config;

// source: https://github.com/vatsimnetwork/vatspy-data-project/blob/a88517cece1e81cd1d18552e7c630e47ddd7739e/Boundaries.geojson?short_path=531fb38#L181
const ZDV_COORDINATES: [(f64, f64); 33] = [
    (-103.166667, 44.958333),
    (-101.483333, 44.7),
    (-101.408333, 43.708333),
    (-100.1, 43.288889),
    (-99.016667, 42.0),
    (-99.058333, 39.983333),
    (-98.8, 39.466667),
    (-102.55, 37.5),
    (-105.0, 36.716667),
    (-106.083333, 36.716667),
    (-107.466667, 36.2),
    (-108.216667, 36.033333),
    (-110.233333, 35.7),
    (-111.841667, 35.766667),
    (-111.504167, 36.420833),
    (-111.608333, 36.733333),
    (-111.879167, 37.4125),
    (-110.883333, 37.833333),
    (-110.156944, 38.129167),
    (-109.983333, 38.2),
    (-109.983333, 38.933333),
    (-109.983333, 39.216667),
    (-110.3, 39.583333),
    (-109.166667, 40.0),
    (-109.1, 40.85),
    (-108.275, 41.366667),
    (-108.0, 41.608333),
    (-107.05, 42.416667),
    (-107.283333, 43.883333),
    (-106.266667, 44.316667),
    (-106.0, 45.2375),
    (-104.25, 45.116667),
    (-103.166667, 44.958333),
];

/// Polygon of ZDV airspace boundaries.
static ZDV_POLYGON: LazyLock<Polygon> =
    LazyLock::new(|| Polygon::new(LineString::from(ZDV_COORDINATES.to_vec()), Vec::new()));

#[derive(Debug, Clone, Default, Serialize, Hash, PartialEq, Eq)]
pub struct OnlineFlight {
    pub pilot_name: String,
    pub pilot_cid: u64,
    pub callsign: String,
    pub departure: String,
    pub arrival: String,
    pub altitude: String,
    pub speed: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OnlineFlights {
    pub plan_within: Vec<OnlineFlight>,
    pub plan_from: Vec<OnlineFlight>,
    pub plan_to: Vec<OnlineFlight>,
    pub actually_within: Vec<OnlineFlight>,
}

/// Return a list of flights that are relevant to the airspace.
///
/// Relevancy is determined as:
///  - Starting at a facility airport
///  - Ending at a facility airport
///  - Starting and ending at a facility airport
///  - Within the facility's airspace
pub fn get_relevant_flights(config: &Config, pilot_data: &[Pilot]) -> OnlineFlights {
    let artcc_fields: Vec<_> = config
        .airports
        .all
        .iter()
        .map(|airport| format!("K{}", airport.code))
        .collect();

    let mut flights = OnlineFlights::default();
    for flight in pilot_data {
        if let Some(plan) = &flight.flight_plan {
            let flight_data = OnlineFlight {
                pilot_name: flight.name.clone(),
                pilot_cid: flight.cid,
                callsign: flight.callsign.clone(),
                departure: plan.departure.clone(),
                arrival: plan.arrival.clone(),
                altitude: flight.altitude.separate_with_commas(),
                speed: flight.groundspeed.separate_with_commas(),
            };
            if ZDV_POLYGON.contains(&Point::new(flight.longitude, flight.latitude)) {
                flights.actually_within.push(flight_data.clone());
            }
            let from = artcc_fields.contains(&plan.departure);
            let to = artcc_fields.contains(&plan.arrival);
            match (from, to) {
                (true, true) => flights.plan_within.push(flight_data),
                (false, true) => flights.plan_to.push(flight_data),
                (true, false) => flights.plan_from.push(flight_data),
                _ => {}
            }
        };
    }

    flights
}

#[cfg(test)]
mod tests {
    use super::ZDV_POLYGON;
    use geo::{Contains, Point};

    #[test]
    fn test_polygon_contains() {
        let contains = ZDV_POLYGON.contains(&Point::new(-107.57511, 39.23782));
        assert!(contains);

        let contains = ZDV_POLYGON.contains(&Point::new(-113.54264, 36.56797));
        assert!(!contains);
    }
}
