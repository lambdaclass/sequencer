use std::fs;

use blockifier::test_utils::contracts::FeatureContract;
use blockifier::test_utils::CairoVersion;
use pretty_assertions::assert_eq;
use rstest::rstest;

const CAIRO0_FEATURE_CONTRACTS_DIR: &str = "feature_contracts/cairo0";
const CAIRO1_FEATURE_CONTRACTS_DIR: &str = "feature_contracts/cairo1";
#[cfg(feature = "cairo_native")]
const NATIVE_FEATURE_CONTRACTS_DIR: &str = "feature_contracts/cairo_native";
const COMPILED_CONTRACTS_SUBDIR: &str = "compiled";
const FIX_COMMAND: &str = "FIX_FEATURE_TEST=1 cargo test -p blockifier --test \
                           feature_contracts_compatibility_test --features testing -- \
                           --include-ignored";

// To fix Cairo0 feature contracts, first enter a python venv and install the requirements:
// ```
// python -m venv tmp_venv
// . tmp_venv/bin/activate
// pip install -r crates/blockifier/tests/requirements.txt
// ```
// Then, run the FIX_COMMAND above.

// To fix Cairo1 feature contracts, first clone the Cairo repo and checkout the required tag.
// The repo should be located next to the sequencer repo:
// <WORKSPACE_DIR>/
// - sequencer/
// - cairo/
// Then, run the FIX_COMMAND above.

// Checks that:
// 1. `TEST_CONTRACTS` dir exists and contains only `.cairo` files and the subdirectory
// `COMPILED_CONTRACTS_SUBDIR`.
// 2. for each `X.cairo` file in `TEST_CONTRACTS` there exists an `X_compiled.json` file in
// `COMPILED_CONTRACTS_SUBDIR` which equals `starknet-compile-deprecated X.cairo --no_debug_info`.
fn verify_feature_contracts_compatibility(fix: bool, cairo_version: CairoVersion) {
    // TODO(Dori, 1/10/2024): Parallelize this test.
    for contract in FeatureContract::all_feature_contracts()
        .filter(|contract| contract.cairo_version() == cairo_version)
    {
        // Compare output of cairo-file on file with existing compiled file.
        let expected_compiled_output = contract.compile();
        let existing_compiled_path = contract.get_compiled_path();

        if fix {
            fs::write(&existing_compiled_path, &expected_compiled_output).unwrap();
        }
        let existing_compiled_contents = fs::read_to_string(&existing_compiled_path)
            .unwrap_or_else(|_| panic!("Cannot read {existing_compiled_path}."));

        if String::from_utf8(expected_compiled_output).unwrap() != existing_compiled_contents {
            panic!(
                "{} does not compile to {existing_compiled_path}.\nRun `{FIX_COMMAND}` to fix the \
                 existing file according to locally installed `starknet-compile-deprecated`.\n",
                contract.get_source_path()
            );
        }
    }
}

/// Verifies that the feature contracts directory contains the expected contents, and returns a list
/// of pairs (source_path, base_filename, compiled_path) for each contract.
fn verify_and_get_files(cairo_version: CairoVersion) -> Vec<(String, String, String)> {
    let mut paths = vec![];
    let directory = match cairo_version {
        CairoVersion::Cairo0 => CAIRO0_FEATURE_CONTRACTS_DIR,
        CairoVersion::Cairo1 => CAIRO1_FEATURE_CONTRACTS_DIR,
        #[cfg(feature = "cairo_native")]
        CairoVersion::Native => NATIVE_FEATURE_CONTRACTS_DIR,
    };
    let compiled_extension = match cairo_version {
        CairoVersion::Cairo0 => "_compiled.json",
        CairoVersion::Cairo1 => ".casm.json",
        #[cfg(feature = "cairo_native")]
        CairoVersion::Native => ".sierra.json",
    };
    for file in fs::read_dir(directory).unwrap() {
        let path = file.unwrap().path();

        // Verify `TEST_CONTRACTS` file and directory structure.
        if !path.is_file() {
            if let Some(dir_name) = path.file_name() {
                assert_eq!(
                    dir_name,
                    COMPILED_CONTRACTS_SUBDIR,
                    "Found directory '{}' in `{directory}`, which should contain only the \
                     `{COMPILED_CONTRACTS_SUBDIR}` directory.",
                    dir_name.to_string_lossy()
                );
                continue;
            }
        }
        let path_str = path.to_string_lossy();
        assert_eq!(
            path.extension().unwrap(),
            "cairo",
            "Found a non-Cairo file '{path_str}' in `{directory}`"
        );

        let file_name = path.file_stem().unwrap().to_string_lossy();
        let existing_compiled_path =
            format!("{directory}/{COMPILED_CONTRACTS_SUBDIR}/{file_name}{compiled_extension}");

        paths.push((path_str.to_string(), file_name.to_string(), existing_compiled_path));
    }

    paths
}

#[test]
fn verify_feature_contracts_match_enum() {
    let mut compiled_paths_from_enum: Vec<String> = FeatureContract::all_feature_contracts()
        .map(|contract| contract.get_compiled_path())
        .collect();
    let mut compiled_paths_on_filesystem: Vec<String> = verify_and_get_files(CairoVersion::Cairo0)
        .into_iter()
        .chain(verify_and_get_files(CairoVersion::Cairo1))
        .map(|(_, _, compiled_path)| compiled_path)
        .collect();
    compiled_paths_from_enum.sort();
    compiled_paths_on_filesystem.sort();
    assert_eq!(compiled_paths_from_enum, compiled_paths_on_filesystem);
}

// todo(rdr): find the right way to feature verify native contracts as well
#[rstest]
#[ignore]
fn verify_feature_contracts(
    #[values(CairoVersion::Cairo0, CairoVersion::Cairo1)] cairo_version: CairoVersion,
) {
    let fix_features = std::env::var("FIX_FEATURE_TEST").is_ok();
    verify_feature_contracts_compatibility(fix_features, cairo_version)
}
