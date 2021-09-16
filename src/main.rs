use chrono::naive::NaiveDate;
use oem_types::condition::ConditionCodeRow;
use oem_types::part::PartNumber;
use oem_types::work_order::*;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::path::Path;
use std::time::Instant;

fn main() {
    let date_cutoff = NaiveDate::from_ymd(2018, 1, 1);

    let dir = Path::new("C:/Users/Fraser Balch/Dropbox/Work/C175 Fuel Rail Life");

    let start = Instant::now();
    let wo_data = WorkOrderData::from_workbook(dir.join("WO Data.xlsb"))
        .unwrap_or_else(|e| panic!("error parsing work order data: {}", e));
    let end = Instant::now();
    println!(
        "work order count: {} ({} ms)",
        wo_data.len(),
        (end - start).as_millis()
    );

    // let start = Instant::now();
    let condition_codes: Vec<_> = ConditionCodeRow::from_dir(dir.join("Condition Codes"))
        .unwrap_or_else(|e| panic!("error parsing condition codes: {}", e))
        .into_iter()
        .map(|row| {
            let parent = wo_data.get_top_parent(row.work_order);
            ConditionCodeRow {
                work_order: parent.work_order,
                finished_good: parent.fg.clone(),
                part_number: row.part_number,
                condition_code: row.condition_code,
            }
        })
        .collect();

    // TODO separate by part life #, 1st 2nd 3rd life reuse (beyond?) for cat reman vs cat new
    // core hours not being read into work order data, check all work order data
    // filtering out work orders where the fuel system was affected
    // graphing library?
    // engine hours and yield rate?
    // is active WIP in the data set?

    let repl = Replacements::new(&wo_data, &condition_codes, |wo| {
        if wo.create_date.date() > date_cutoff
            && wo.work_type.is_standard_job()
            && wo.sales_level == SalesLevel::BeforeFailure
        // && wo.previous_work_order == Some(PreviousWorkOrder::CatNew)
        {
            C175Tier2::try_from(&wo.fg)
                .ok()
                .map(|c| (c, wo.core_hours.map(|hr| hr / 1000)))
            // .map(|c| (c, chrono::Datelike::year(&wo.create_date)))
        } else {
            None
        }
    });

    println!();
    println!("{}", repl);
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum C175Tier2 {
    C175_16,
    C175_20,
}

impl<'a> TryFrom<&'a FinishedGood> for C175Tier2 {
    type Error = &'a FinishedGood;

    fn try_from(fg: &'a FinishedGood) -> Result<Self, Self::Error> {
        if fg.contains("2863503")
            || fg.contains("3659327")
            || fg.contains("5111398")
            || fg.contains("5193974")
            || fg.contains("5144913")
        {
            Ok(C175Tier2::C175_20)
        } else if fg.contains("3659319") || fg.contains("5632124") {
            Ok(C175Tier2::C175_16)
        } else {
            Err(fg)
        }
    }
}

impl std::fmt::Display for C175Tier2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            C175Tier2::C175_16 => "C175-16",
            C175Tier2::C175_20 => "C175-20",
        };
        write!(f, "{}", str)
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
struct Replacement {
    replacements: u32,
    wo_count: u32,
}

impl Replacement {
    pub fn rate(&self) -> f32 {
        self.replacements as f32 / self.wo_count as f32
    }
}

impl Display for Replacement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:.0}% ({} / {})",
            self.rate() * 100.0,
            self.replacements,
            self.wo_count
        )
    }
}

#[derive(Debug, Default, Clone)]
struct Replacements<T> {
    map: HashMap<ReplacementKey<T>, Replacement>,
}

impl<'a, T: Eq + Hash> Replacements<T> {
    pub fn new<F>(
        wo_data: &'a WorkOrderData,
        condition_codes: &Vec<ConditionCodeRow>,
        try_key: F,
    ) -> Self
    where
        F: Fn(&WorkOrderRow) -> Option<T>,
    {
        let mut map = HashMap::<_, Replacement>::default();

        // increment parts replaced
        for cc in condition_codes.iter() {
            let parent = wo_data.get_top_parent(cc.work_order);
            if let Some(t) = try_key(parent) {
                let part = cc.part_number.clone();
                let key = ReplacementKey { t, part };
                map.entry(key).or_default().replacements += 1;
            }
        }

        // increment matching work orders
        for wo in wo_data.iter() {
            if wo.parent.is_none() {
                if let Some(t) = try_key(wo) {
                    map.iter_mut()
                        .filter(|(k, _)| k.t == t)
                        .for_each(|(_, v)| v.wo_count += 1);
                }
            }
        }

        Self { map }
    }
}

impl<T: Debug + Ord> Display for Replacements<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut vec: Vec<_> = self.map.iter().collect();
        vec.sort_by(|a, b| a.0.cmp(&b.0));

        writeln!(f, "Engine Part Replacements:")?;
        for (wo_part, repl) in vec.iter() {
            writeln!(f, "  {:?} {}: {}", &wo_part.t, &wo_part.part, repl)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ReplacementKey<T> {
    t: T,
    part: PartNumber,
}
