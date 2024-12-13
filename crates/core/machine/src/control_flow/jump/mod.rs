mod air;
mod columns;
mod trace;

pub use columns::*;
use p3_air::BaseAir;

#[derive(Default)]
pub struct JumpChip;

impl<F> BaseAir<F> for JumpChip {
    fn width(&self) -> usize {
        NUM_JUMP_COLS
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::BorrowMut;

    use p3_baby_bear::BabyBear;
    use p3_field::AbstractField;
    use p3_matrix::dense::RowMajorMatrix;
    use sp1_core_executor::{ExecutionRecord, Instruction, Opcode, Program};
    use sp1_stark::{
        air::MachineAir, baby_bear_poseidon2::BabyBearPoseidon2, CpuProver, MachineProver, Val,
    };

    use crate::{
        control_flow::{JumpChip, JumpColumns},
        io::SP1Stdin,
        riscv::RiscvAir,
        utils::run_malicious_test,
    };

    #[test]
    fn test_malicious_jal() {
        let instructions = vec![
            Instruction::new(Opcode::JAL, 29, 12, 0, true, true),
            Instruction::new(Opcode::ADD, 30, 0, 5, false, true),
            Instruction::new(Opcode::ADD, 28, 0, 5, false, true),
            Instruction::new(Opcode::ADD, 28, 0, 5, false, true),
        ];
        let program = Program::new(instructions, 0, 0);
        let stdin = SP1Stdin::new();

        type P = CpuProver<BabyBearPoseidon2, RiscvAir<BabyBear>>;

        let malicious_trace_pv_generator =
            |prover: &P,
             record: &mut ExecutionRecord|
             -> Vec<(String, RowMajorMatrix<Val<BabyBearPoseidon2>>)> {
                // Create a malicious record where the BEQ instruction branches incorrectly.
                let mut malicious_record = record.clone();
                malicious_record.cpu_events[0].next_pc = 4;
                malicious_record.jump_events[0].next_pc = 4;
                prover.generate_traces(&malicious_record)
            };

        let result =
            run_malicious_test::<P>(program, stdin, Box::new(malicious_trace_pv_generator));
        assert!(result.is_err() && result.unwrap_err().is_constraints_failing());
    }

    #[test]
    fn test_malicious_jalr() {
        let instructions = vec![
            Instruction::new(Opcode::ADD, 28, 0, 12, false, true),
            Instruction::new(Opcode::JALR, 29, 28, 0, false, true),
            Instruction::new(Opcode::ADD, 30, 0, 5, false, true),
            Instruction::new(Opcode::ADD, 28, 0, 5, false, true),
            Instruction::new(Opcode::ADD, 28, 0, 5, false, true),
        ];
        let program = Program::new(instructions, 0, 0);
        let stdin = SP1Stdin::new();

        type P = CpuProver<BabyBearPoseidon2, RiscvAir<BabyBear>>;

        let malicious_trace_pv_generator =
            |prover: &P,
             record: &mut ExecutionRecord|
             -> Vec<(String, RowMajorMatrix<Val<BabyBearPoseidon2>>)> {
                // Create a malicious record where the BEQ instruction branches incorrectly.
                let mut malicious_record = record.clone();
                malicious_record.cpu_events[1].next_pc = 8;
                malicious_record.jump_events[0].next_pc = 8;
                prover.generate_traces(&malicious_record)
            };

        let result =
            run_malicious_test::<P>(program, stdin, Box::new(malicious_trace_pv_generator));
        assert!(result.is_err() && result.unwrap_err().is_constraints_failing());
    }

    #[test]
    fn test_malicious_multiple_opcode_flags() {
        let instructions = vec![
            Instruction::new(Opcode::JAL, 29, 12, 0, true, true),
            Instruction::new(Opcode::ADD, 30, 0, 5, false, true),
            Instruction::new(Opcode::ADD, 28, 0, 5, false, true),
            Instruction::new(Opcode::ADD, 28, 0, 5, false, true),
        ];
        let program = Program::new(instructions, 0, 0);
        let stdin = SP1Stdin::new();

        type P = CpuProver<BabyBearPoseidon2, RiscvAir<BabyBear>>;

        let malicious_trace_pv_generator =
            |prover: &P,
             record: &mut ExecutionRecord|
             -> Vec<(String, RowMajorMatrix<Val<BabyBearPoseidon2>>)> {
                // Modify the branch chip to have a row that has multiple opcode flags set.
                let mut traces = prover.generate_traces(record);
                let jump_chip_name = <JumpChip as MachineAir<BabyBear>>::name(&JumpChip {});
                for (chip_name, trace) in traces.iter_mut() {
                    if *chip_name == jump_chip_name {
                        let first_row = trace.row_mut(0);
                        let first_row: &mut JumpColumns<BabyBear> = first_row.borrow_mut();
                        assert!(first_row.is_jal == BabyBear::one());
                        first_row.is_jalr = BabyBear::one();
                    }
                }
                traces
            };

        let result =
            run_malicious_test::<P>(program, stdin, Box::new(malicious_trace_pv_generator));
        assert!(result.is_err() && result.unwrap_err().is_constraints_failing());
    }
}
