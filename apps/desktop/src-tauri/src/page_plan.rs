use std::collections::HashSet;

use chrono::{SecondsFormat, Utc};
use sha2::{Digest, Sha256};

use crate::contracts::{
    CorePdfPlanPayload, OperationError, OperationPlanEnvelope, OperationStage, OutputRotation,
    StoredOperationPlan, CORE_PDF_MAX_PAGES, OPERATION_PLAN_MAX_BYTES,
    OPERATION_PLAN_SCHEMA_VERSION, PDF_EXTRACT_OPERATION_ID, PDF_REMOVE_OPERATION_ID,
    PDF_REORDER_OPERATION_ID, PDF_ROTATE_OPERATION_ID, PDF_SPLIT_MAX_OUTPUTS,
    PDF_SPLIT_OPERATION_ID,
};
use crate::path_policy::validate_output_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOutput {
    pub output_name: String,
    pub page_indexes: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPagePlan {
    pub stored: StoredOperationPlan,
    pub outputs: Vec<PlannedOutput>,
    pub rotations: Vec<(u32, OutputRotation)>,
}

pub fn validate_plan(envelope: OperationPlanEnvelope) -> Result<ValidatedPagePlan, OperationError> {
    if envelope.schema_version != OPERATION_PLAN_SCHEMA_VERSION
        || envelope.source_page_count == 0
        || envelope.source_page_count > CORE_PDF_MAX_PAGES
    {
        return Err(invalid_plan());
    }
    let page_count = envelope.source_page_count;
    let all_pages = || (0..page_count).collect::<Vec<_>>();
    let (outputs, rotations) = match (&envelope.operation_id[..], &envelope.payload) {
        (PDF_EXTRACT_OPERATION_ID, CorePdfPlanPayload::Extract(plan)) => {
            validate_output(&plan.output_name)?;
            validate_unique_indexes(&plan.selected_page_indexes, page_count, false)?;
            (
                vec![PlannedOutput {
                    output_name: plan.output_name.clone(),
                    page_indexes: plan.selected_page_indexes.clone(),
                }],
                Vec::new(),
            )
        }
        (PDF_REMOVE_OPERATION_ID, CorePdfPlanPayload::Remove(plan)) => {
            validate_output(&plan.output_name)?;
            validate_unique_indexes(&plan.removed_page_indexes, page_count, false)?;
            if plan.removed_page_indexes.len() >= page_count as usize {
                return Err(invalid_plan());
            }
            let removed = plan
                .removed_page_indexes
                .iter()
                .copied()
                .collect::<HashSet<_>>();
            (
                vec![PlannedOutput {
                    output_name: plan.output_name.clone(),
                    page_indexes: all_pages()
                        .into_iter()
                        .filter(|index| !removed.contains(index))
                        .collect(),
                }],
                Vec::new(),
            )
        }
        (PDF_REORDER_OPERATION_ID, CorePdfPlanPayload::Reorder(plan)) => {
            validate_output(&plan.output_name)?;
            validate_unique_indexes(&plan.ordered_page_indexes, page_count, true)?;
            (
                vec![PlannedOutput {
                    output_name: plan.output_name.clone(),
                    page_indexes: plan.ordered_page_indexes.clone(),
                }],
                Vec::new(),
            )
        }
        (PDF_ROTATE_OPERATION_ID, CorePdfPlanPayload::Rotate(plan)) => {
            validate_output(&plan.output_name)?;
            if plan.rotations.is_empty() || plan.rotations.len() > page_count as usize {
                return Err(invalid_plan());
            }
            let mut seen = HashSet::with_capacity(plan.rotations.len());
            let mut rotations = Vec::with_capacity(plan.rotations.len());
            for rotation in &plan.rotations {
                if rotation.page_index >= page_count || !seen.insert(rotation.page_index) {
                    return Err(invalid_plan());
                }
                rotations.push((rotation.page_index, rotation.clockwise_degrees));
            }
            (
                vec![PlannedOutput {
                    output_name: plan.output_name.clone(),
                    page_indexes: all_pages(),
                }],
                rotations,
            )
        }
        (PDF_SPLIT_OPERATION_ID, CorePdfPlanPayload::Split(plan)) => {
            if plan.ranges.is_empty() || plan.ranges.len() > PDF_SPLIT_MAX_OUTPUTS {
                return Err(invalid_plan());
            }
            let mut names = HashSet::with_capacity(plan.ranges.len());
            let mut expected_start = 0_u32;
            let mut outputs = Vec::with_capacity(plan.ranges.len());
            for range in &plan.ranges {
                validate_output(&range.output_name)?;
                if !names.insert(range.output_name.to_ascii_lowercase())
                    || range.start_page_index != expected_start
                    || range.end_page_index < range.start_page_index
                    || range.end_page_index >= page_count
                {
                    return Err(invalid_plan());
                }
                outputs.push(PlannedOutput {
                    output_name: range.output_name.clone(),
                    page_indexes: (range.start_page_index..=range.end_page_index).collect(),
                });
                expected_start = range
                    .end_page_index
                    .checked_add(1)
                    .ok_or_else(invalid_plan)?;
            }
            if expected_start != page_count {
                return Err(invalid_plan());
            }
            (outputs, Vec::new())
        }
        _ => return Err(invalid_plan()),
    };

    let canonical_json = serde_json::to_string(&envelope).map_err(|_| invalid_plan())?;
    if canonical_json.len() < 2 || canonical_json.len() > OPERATION_PLAN_MAX_BYTES {
        return Err(invalid_plan());
    }
    let sha256 = hex_digest(&Sha256::digest(canonical_json.as_bytes()));
    Ok(ValidatedPagePlan {
        stored: StoredOperationPlan {
            envelope,
            canonical_json,
            sha256,
            created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        },
        outputs,
        rotations,
    })
}

fn validate_unique_indexes(
    indexes: &[u32],
    page_count: u32,
    require_permutation: bool,
) -> Result<(), OperationError> {
    if indexes.is_empty()
        || indexes.len() > page_count as usize
        || (require_permutation && indexes.len() != page_count as usize)
    {
        return Err(invalid_plan());
    }
    let mut seen = HashSet::with_capacity(indexes.len());
    if indexes
        .iter()
        .any(|index| *index >= page_count || !seen.insert(*index))
    {
        return Err(invalid_plan());
    }
    Ok(())
}

fn validate_output(name: &str) -> Result<(), OperationError> {
    validate_output_name(name).map_err(|_| invalid_plan())?;
    if !name.to_ascii_lowercase().ends_with(".pdf") {
        return Err(invalid_plan());
    }
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn invalid_plan() -> OperationError {
    OperationError::safe(
        "PAGE_PLAN_INVALID",
        "The page operation plan is not valid",
        "Review the selected pages, order, rotations, split ranges, and output names.",
        OperationStage::Plan,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::validate_plan;
    use crate::contracts::{
        CorePdfPlanPayload, OperationPlanEnvelope, ReorderPagesPlan, SplitOutputRange, SplitPlan,
    };

    #[test]
    fn exact_reorder_permutation_is_accepted() {
        let plan = validate_plan(OperationPlanEnvelope {
            schema_version: 1,
            operation_id: "pdf.reorder-pages".to_owned(),
            source_page_count: 3,
            payload: CorePdfPlanPayload::Reorder(ReorderPagesPlan {
                ordered_page_indexes: vec![2, 0, 1],
                output_name: "reordered.pdf".to_owned(),
            }),
        })
        .unwrap();
        assert_eq!(plan.outputs[0].page_indexes, [2, 0, 1]);
        assert_eq!(plan.stored.sha256.len(), 64);
        assert!(plan.stored.canonical_json.len() <= 65_536);
    }

    #[test]
    fn split_must_partition_the_entire_source_without_gaps() {
        let error = validate_plan(OperationPlanEnvelope {
            schema_version: 1,
            operation_id: "pdf.split".to_owned(),
            source_page_count: 4,
            payload: CorePdfPlanPayload::Split(SplitPlan {
                ranges: vec![
                    SplitOutputRange {
                        start_page_index: 0,
                        end_page_index: 0,
                        output_name: "part-1.pdf".to_owned(),
                    },
                    SplitOutputRange {
                        start_page_index: 2,
                        end_page_index: 3,
                        output_name: "part-2.pdf".to_owned(),
                    },
                ],
            }),
        })
        .unwrap_err();
        assert_eq!(error.code, "PAGE_PLAN_INVALID");
    }
}
