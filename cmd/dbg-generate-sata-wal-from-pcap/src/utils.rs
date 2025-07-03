//! Utility functions that don't have one place that they should live.

use miette::Report;

/// Add context to a specific error, where you can have like a list of
/// suggestions.
///
/// NOTE: we cannot reassign a reports severity, so your last items severity
///       is where the real severity gets taken.
pub fn add_context_to(
	original_error: Report,
	suggestions: impl DoubleEndedIterator<Item = Report>,
) -> Report {
	let mut latest_error: Option<Report> = None;

	for suggestion in suggestions.rev() {
		if let Some(last_error) = latest_error {
			latest_error = Some(last_error.wrap_err(suggestion));
		} else {
			latest_error = Some(suggestion);
		}
	}

	if let Some(latest) = latest_error {
		latest.wrap_err(original_error)
	} else {
		original_error
	}
}
