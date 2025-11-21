
#[cfg(test)]
mod reproduction_test {
    use crate::kmer::*;
    use std::collections::HashMap;

    fn to_bytes(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[test]
    fn test_primer_with_homopolymer_assembly() {
        // Simulate a primer that has a homopolymer region
        // Primer: ATCG AAAAAAAA GCTAG
        // k=8
        // k-mers:
        // ATCGAAAA
        // TCGAAAAA
        // CGAAAAAA
        // GAAAAAAA (Homopolymer! 8/8 A)
        // AAAAAAAA (Homopolymer! 8/8 A)
        // AAAAAAAG (Homopolymer! 7/8 A -> 87.5%)
        // AAAAAAGC
        // AAAAAGCT
        // AAAAGCTA
        // AAAGCTAG
        
        let k = 8;
        let mut kmers = HashMap::new();
        
        let seqs = vec![
            "ATCGAAAA",
            "TCGAAAAA",
            "CGAAAAAA",
            "GAAAAAAA", // Likely filtered as homopolymer
            "AAAAAAAA", // Likely filtered as homopolymer
            "AAAAAAAG", // Likely filtered as homopolymer
            "AAAAAAGC",
            "AAAAAGCT",
            "AAAAGCTA",
            "AAAGCTAG"
        ];

        for s in seqs {
            let bytes = to_bytes(s);
            let is_homo = is_homopolymer(&bytes);
            println!("{} is_homopolymer: {}", s, is_homo);
            kmers.insert(bytes.clone(), KmerStats::new(bytes, 100, 10.0));
        }

        // Current implementation filters out homopolymers before assembly
        let assembled = assemble_kmers(&kmers, k, 0.8);
        
        println!("Assembled: {:?}", assembled.keys().map(|k| String::from_utf8_lossy(k)).collect::<Vec<_>>());

        // We expect the primer to be broken or not assembled fully if homopolymers are filtered
        // The full primer is ATCGAAAAAAAAGCTAG (17 bp)
        // If GAAAAAAA, AAAAAAAA, AAAAAAAG are filtered, we lose the middle connection.
        
        let full_primer = "ATCGAAAAAAAAGCTAG";
        let assembled_strings: Vec<String> = assembled.keys().map(|k| String::from_utf8_lossy(k).to_string()).collect();
        
        let found = assembled_strings.iter().any(|s| s.contains(full_primer) || full_primer.contains(s) && s.len() > 10);
        
        if !found {
            println!("FAILED to assemble full primer due to homopolymer filtering");
        } else {
            println!("SUCCESSfully assembled primer");
        }
    }

    #[test]
    fn test_true_branching_preserved() {
        // Scenario: Adapter A ends in ...ATCG
        // Adapter B starts with GCTA...
        // Adapter C starts with CCTA...
        // k=4
        // A_end: ATCG
        // B_start: TCGA -> CGAT -> GCTA
        // C_start: TCGA -> CGAC -> CCTA
        // Common k-mer: TCGA
        // Next k-mers: CGAT (for B), CGAC (for C)
        // There should be NO edge between CGAT and CGAC.
        
        let k = 4;
        let mut kmers = HashMap::new();
        
        let seqs = vec![
            "ATCG", // ... -> ATCG
            "TCGA", // ATCG -> TCGA
            "CGAT", // TCGA -> CGAT (Path B)
            "GCTA", // CGAT -> GCTA
            "CGAC", // TCGA -> CGAC (Path C)
            "CCTA", // CGAC -> CCTA
        ];
        
        for s in seqs {
            let bytes = to_bytes(s);
            kmers.insert(bytes.clone(), KmerStats::new(bytes, 100, 10.0));
        }
        
        // We expect assembly to stop at TCGA because it branches to CGAT and CGAC
        // and neither is a transitive neighbor of the other.
        
        let assembled = assemble_kmers(&kmers, k, 0.8);
        
        println!("Assembled sequences:");
        for (seq, _) in &assembled {
            println!("  {}", String::from_utf8_lossy(seq));
        }
        
        // Should find "ATCGA" (ATCG -> TCGA)
        // And maybe "CGATGCTA" and "CGACCCTA" if they are considered starts?
        // "TCGA" is a branch point.
        // If we start at ATCG: ATCG -> TCGA -> STOP. Result: ATCGA.
        
        let assembled_strings: Vec<String> = assembled.keys().map(|k| String::from_utf8_lossy(k).to_string()).collect();
        
        assert!(assembled_strings.contains(&"ATCGA".to_string()));
        
        // Ensure we did NOT merge them into something weird or pick one arbitrarily
        let merged_B = assembled_strings.iter().any(|s| s.contains("ATCGATGCTA"));
        let merged_C = assembled_strings.iter().any(|s| s.contains("ATCGACCTA"));
        
        assert!(!merged_B, "Should not assemble past branch point (Path B)");
        assert!(!merged_C, "Should not assemble past branch point (Path C)");
    }
}
