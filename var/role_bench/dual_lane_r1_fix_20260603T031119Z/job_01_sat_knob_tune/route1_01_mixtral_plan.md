## Plan

Our goal is to improve the blue-green clarity scoring for the Cesarops-Satellite POC during the clear-water Straits seasons. To achieve this, we will focus on adjusting the knobs in the types.rs file and modifying the mission spec. Here's our plan:

1. **Knob Adjustment**: Analyze the current knob settings in the types.rs file and identify areas for improvement. We will focus on adjusting the z-score thresholds for the blue and green bands to better match the clear-water Straits seasons.
2. **Mission Spec Modification**: Modify the mission spec to incorporate the adjusted knob settings. This will ensure that the new settings are applied consistently across all missions.
3. **Testing**: Test the new knob settings with three specific test commands. This will allow us to validate the changes and ensure that they are working as intended.

## Acceptance

To ensure that our plan is successful, we need to meet the following acceptance criteria:

1. **Knob Adjustment**: The adjusted knob settings in the types.rs file must result in improved blue-green clarity scoring during the clear-water Straits seasons. We will measure success by comparing the scoring before and after the knob adjustments.
2. **Mission Spec Modification**: The modified mission spec must correctly apply the adjusted knob settings. We will verify this by running test missions and comparing the results with and without the modified mission spec.
3. **Testing**: The three test commands must validate the knob adjustments and mission spec modifications. We will consider the plan successful if the test commands show consistent improvement in the blue-green clarity scoring during the clear-water Straits seasons.

## Test Commands

1. `cargo run -- clear-water-straits -- knob-settings=adjusted`
2. `cargo test -- mission-spec=modified -- clear-water-straits`
3. `cargo compare -- before=original -- after=modified -- clear-water-straits`

These test commands will allow us to test the knob adjustments, mission spec modifications, and compare the results with the original settings. By focusing on these specific tests, we can ensure that our changes are effective and reliable.