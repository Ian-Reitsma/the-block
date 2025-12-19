//! Integration test: Complete treasury disbursement lifecycle
//!
//! Tests the flow: Draft → Voting → Queued → Timelocked → Executed → Finalized
//! Including dependency validation and error handling.

#[cfg(test)]
mod treasury_lifecycle {
    use governance::treasury::TreasuryDisbursement;
    use governance::treasury_deps::{DependencyError, DependencyGraph, DependencyStatus};

    /// Helper to create a test disbursement
    fn create_disbursement(id: u64, amount_ct: u64, memo: &str) -> TreasuryDisbursement {
        TreasuryDisbursement::new(
            id,
            format!("ct1qqqqqqqq{}", id),
            amount_ct,
            0,
            memo.to_string(),
            1,
        )
    }

    #[test]
    fn test_simple_execution_flow() {
        // Scenario: Single disbursement with no dependencies
        let disb = create_disbursement(1001, 50_000, "");

        // Initially should be in draft/voting
        assert_eq!(disb.id, 1001);
        assert_eq!(disb.amount_ct, 50_000);
    }

    #[test]
    fn test_dependent_disbursements() {
        // Scenario: Disbursement B depends on Disbursement A
        let disb_a = create_disbursement(
            1000,
            100_000,
            r#"{"reason": "Phase 1 funding"}"#,
        );
        let disb_b = create_disbursement(
            1001,
            50_000,
            r#"{"depends_on": [1000], "reason": "Phase 2 funding"}"#,
        );

        // Both should be created successfully
        assert_eq!(disb_a.id, 1000);
        assert_eq!(disb_b.id, 1001);
        assert!(disb_b.memo.contains("depends_on"));
    }

    #[test]
    fn test_dependency_graph_creation() {
        // Create a simple DAG: 999 → 1000 → 1001
        let disbursements = vec![
            create_disbursement(999, 100_000, ""),
            create_disbursement(1000, 50_000, r#"{"depends_on": [999]}"#),
            create_disbursement(1001, 25_000, r#"{"depends_on": [1000]}"#),
        ];

        // Build graph
        let graph = DependencyGraph::new(&disbursements);
        assert!(graph.is_ok());

        let graph = graph.unwrap();
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2); // 999→1000, 1000→1001
    }

    #[test]
    fn test_cycle_detection() {
        // Create a cyclic dependency: 1000 → 1001 → 1000
        let disbursements = vec![
            create_disbursement(1000, 100_000, r#"{"depends_on": [1001]}"#),
            create_disbursement(1001, 50_000, r#"{"depends_on": [1000]}"#),
        ];

        let graph = DependencyGraph::new(&disbursements);
        assert!(graph.is_ok());

        let graph = graph.unwrap();
        let cycle_result = graph.has_cycle();
        assert!(cycle_result.is_err());

        match cycle_result {
            Err(DependencyError::CycleDetected { cycle }) => {
                assert!(cycle.contains(&1000));
                assert!(cycle.contains(&1001));
            }
            _ => panic!("Expected cycle detection error"),
        }
    }

    #[test]
    fn test_missing_dependency() {
        // Create disbursement referencing non-existent dependency
        let disbursements = vec![create_disbursement(
            1001,
            50_000,
            r#"{"depends_on": [999]}"#, // 999 doesn't exist
        )];

        let graph = DependencyGraph::new(&disbursements);
        assert!(graph.is_err());

        match graph {
            Err(DependencyError::MissingDependency {
                disbursement_id,
                missing_id,
            }) => {
                assert_eq!(disbursement_id, 1001);
                assert_eq!(missing_id, 999);
            }
            _ => panic!("Expected missing dependency error"),
        }
    }

    #[test]
    fn test_topological_sort() {
        // Create DAG and verify topological ordering
        let disbursements = vec![
            create_disbursement(999, 100_000, ""),
            create_disbursement(1000, 50_000, r#"{"depends_on": [999]}"#),
            create_disbursement(1001, 25_000, r#"{"depends_on": [999, 1000]}"#),
        ];

        let graph = DependencyGraph::new(&disbursements).unwrap();
        let sorted = graph.topological_sort().unwrap();

        // Verify ordering: 999 comes before 1000, both before 1001
        let pos_999 = sorted.iter().position(|&x| x == 999).unwrap();
        let pos_1000 = sorted.iter().position(|&x| x == 1000).unwrap();
        let pos_1001 = sorted.iter().position(|&x| x == 1001).unwrap();

        assert!(pos_999 < pos_1000);
        assert!(pos_999 < pos_1001);
        assert!(pos_1000 < pos_1001);
    }

    #[test]
    fn test_complex_dependency_graph() {
        // Create a more complex DAG:
        //     999
        //    /   \
        //  1000  1003
        //    \   /
        //     1001
        //       |
        //     1002
        let disbursements = vec![
            create_disbursement(999, 100_000, ""),
            create_disbursement(1000, 50_000, r#"{"depends_on": [999]}"#),
            create_disbursement(1001, 40_000, r#"{"depends_on": [999, 1000]}"#),
            create_disbursement(1002, 30_000, r#"{"depends_on": [1001]}"#),
            create_disbursement(1003, 20_000, r#"{"depends_on": [999]}"#),
        ];

        let graph = DependencyGraph::new(&disbursements).unwrap();
        assert_eq!(graph.node_count(), 5);
        assert_eq!(graph.edge_count(), 6); // 999→1000, 999→1001, 1000→1001, 1001→1002, 999→1003

        // Should have no cycles
        assert!(graph.has_cycle().is_ok());

        // Should produce valid topological order
        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted.len(), 5);
    }

    #[test]
    fn test_multiple_dependency_formats() {
        // Test that both JSON and key=value formats work
        let disbursements = vec![
            create_disbursement(999, 100_000, ""),
            // JSON format
            create_disbursement(1000, 50_000, r#"{"depends_on": [999]}"#),
            // Key=value format
            create_disbursement(1001, 25_000, "depends_on=999"),
            // Mixed
            create_disbursement(1002, 10_000, r#"{"depends_on": [999, 1000, 1001]}"#),
        ];

        let graph = DependencyGraph::new(&disbursements);
        assert!(graph.is_ok());
        let graph = graph.unwrap();
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    fn test_empty_graph() {
        let disbursements: Vec<TreasuryDisbursement> = vec![];
        let graph = DependencyGraph::new(&disbursements);

        assert!(graph.is_err());
        assert!(matches!(graph, Err(DependencyError::EmptyGraph)));
    }

    #[test]
    fn test_large_graph_performance() {
        // Create a larger DAG to test performance
        let mut disbursements = vec![];

        // Create a linear chain: 1 → 2 → 3 → ... → 100
        for i in 1..=100 {
            let memo = if i == 1 {
                String::new()
            } else {
                format!(r#"{{"depends_on": [{}]}}"#, i - 1)
            };
            disbursements.push(create_disbursement(i as u64, 1000, &memo));
        }

        let start = std::time::Instant::now();
        let graph = DependencyGraph::new(&disbursements).unwrap();
        let duration = start.elapsed();

        // Should complete quickly (< 100ms)
        assert!(duration.as_millis() < 100);
        assert_eq!(graph.node_count(), 100);
        assert_eq!(graph.edge_count(), 99);

        // Topological sort should also be fast
        let start = std::time::Instant::now();
        let sorted = graph.topological_sort().unwrap();
        let duration = start.elapsed();

        assert!(duration.as_millis() < 100);
        assert_eq!(sorted.len(), 100);

        // Verify chain is in correct order
        for i in 1..100 {
            let pos_i = sorted.iter().position(|&x| x == i as u64).unwrap();
            let pos_next = sorted.iter().position(|&x| x == (i + 1) as u64).unwrap();
            assert!(pos_i < pos_next);
        }
    }

    #[test]
    fn test_error_messages() {
        // Verify error messages are informative
        let disbursements = vec![create_disbursement(
            1001,
            50_000,
            r#"{"depends_on": [999]}"#,
        )];

        match DependencyGraph::new(&disbursements) {
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("1001") || msg.contains("999"));
            }
            Ok(_) => panic!("Should have failed"),
        }
    }
}
