use anyhow::Result;
use chrono::{Duration, Local};

use crate::db::Db;
use crate::models::Status;

/// Populate the database with sample data for demo purposes.
/// Sprint is modelled as halfway through (started 3 days ago, ends in 3 days).
pub fn seed(db: &Db) -> Result<()> {
    let today = Local::now().date_naive();

    // Sprint runs from 3 days ago to 3 days from now (7-day sprint)
    let sprint_start = today - Duration::days(3);
    let sprint_end   = today + Duration::days(3);

    let sprint_id = db.create_sprint("Sprint 4", sprint_start, sprint_end, true)?;

    // Helper: format a past datetime string N days before today
    let days_ago = |n: i64| -> String {
        let d = today - Duration::days(n);
        format!("{} 10:00:00", d.format("%Y-%m-%d"))
    };

    // ── Sprint issues ─────────────────────────────────────────────────────────
    // Done: 3 issues completed on different days → burndown shows progress

    let i1 = db.create_issue(
        "Write VASP INCAR/KPOINTS templates",
        1.0,
        "setup",
        &Status::Done,
        None,
        Some("Create reusable INCAR templates for relaxation, static, and DOS runs. Set EDIFF=1e-6, EDIFFG=-0.01. Document each tag with a comment."),
    )?;
    db.set_issue_sprint(i1, Some(sprint_id))?;
    db.set_completed_at(i1, &days_ago(3))?;

    let i2 = db.create_issue(
        "Compile VASP 6.4 on Negishi HPC",
        2.0,
        "setup",
        &Status::Done,
        None,
        Some("Build VASP with Intel MKL and OpenMPI. Enable CUDA offloading for V100 nodes. Confirm single-point energy for bulk Ti3C2 matches reference."),
    )?;
    db.set_issue_sprint(i2, Some(sprint_id))?;
    db.set_completed_at(i2, &days_ago(2))?;

    let i3 = db.create_issue(
        "Geometry relax Ti3C2Tx unit cell",
        3.0,
        "dft",
        &Status::Done,
        None,
        Some("ISIF=3 cell+ion relaxation with PBE-D3(BJ) dispersion. Converge to EDIFFG=-0.01 eV/Å. Starting from experimental lattice parameters (a=3.05 Å, c=19.8 Å)."),
    )?;
    db.set_issue_sprint(i3, Some(sprint_id))?;
    db.set_completed_at(i3, &days_ago(1))?;

    // In progress
    let i4 = db.create_issue(
        "Converge ENCUT for Ti3C2 slab",
        2.0,
        "dft",
        &Status::InProgress,
        None,
        Some("Sweep ENCUT from 400 to 700 eV in 50 eV steps. Target total energy convergence < 1 meV/atom. Use 3-layer slab with 15 Å vacuum."),
    )?;
    db.set_issue_sprint(i4, Some(sprint_id))?;

    let i5 = db.create_issue(
        "Converge k-point mesh for surface BZ",
        2.0,
        "dft",
        &Status::InProgress,
        None,
        Some("Test 6x6x1, 9x9x1, 12x12x1 Gamma-centred meshes. Check total energy and DOS convergence. Avoid k-points along high-symmetry directions that cause discontinuities."),
    )?;
    db.set_issue_sprint(i5, Some(sprint_id))?;

    // Todo
    let i6 = db.create_issue(
        "Run AIMD at 300 K for 10 ps",
        5.0,
        "aimd",
        &Status::Todo,
        Some(sprint_end),
        Some("NVT ensemble with Nosé-Hoover thermostat (SMASS=0). POTIM=2 fs, NBLOCK=1. 5000 steps. Confirm equilibration via temperature and pressure fluctuations."),
    )?;
    db.set_issue_sprint(i6, Some(sprint_id))?;

    let i7 = db.create_issue(
        "Extract MSD from AIMD trajectory",
        2.0,
        "analysis",
        &Status::Todo,
        Some(sprint_end),
        Some("Use VASPKIT or pyiron to parse XDATCAR. Plot MSD vs time. Estimate self-diffusion coefficient via Einstein relation."),
    )?;
    db.set_issue_sprint(i7, Some(sprint_id))?;

    // ── Backlog ───────────────────────────────────────────────────────────────
    db.create_issue(
        "Calculate phonon dispersion (phonopy)",
        5.0,
        "phonon",
        &Status::Todo,
        Some(today + Duration::days(14)),
        Some("Use finite-displacement method. 3x3x1 supercell, 0.01 Å displacement. Compare acoustic branches at Gamma with Raman shifts from experiment. Export FORCE_SETS for further analysis."),
    )?;

    db.create_issue(
        "Benchmark PBE vs HSE06 bandgap",
        4.0,
        "dft",
        &Status::Todo,
        None,
        Some("Compute band structure with PBE and HSE06 (HFSCREEN=0.2). Compare Fermi level alignment. Expected metallic character for bare MXene; O-terminated may gap."),
    )?;

    db.create_issue(
        "Generate O/OH/F surface terminations",
        3.0,
        "dft",
        &Status::Todo,
        Some(today + Duration::days(21)),
        Some("Build all three termination types from relaxed bare Ti3C2. Run ISIF=2 relaxation for each. Compare adsorption energies and surface charge density."),
    )?;

    db.create_issue(
        "Write group meeting slides",
        2.0,
        "writing",
        &Status::Todo,
        Some(today + Duration::days(5)),
        Some("15 min slot. Cover: motivation, structure, AIMD setup, preliminary results. Include convergence plots and any issues with HPC queue limits."),
    )?;

    db.create_issue(
        "Draft manuscript for Chem. Mater.",
        8.0,
        "writing",
        &Status::Todo,
        Some(today + Duration::days(28)),
        Some("Target: MXene thermal stability mechanism. Sections: intro, methods (VASP/AIMD), results (MSD, phonon, termination effects), discussion, conclusion. Aim for ~6000 words."),
    )?;

    db.create_issue(
        "Fit NequIP ML interatomic potential",
        5.0,
        "mlmd",
        &Status::Todo,
        None,
        Some("Train NequIP on DFT force/energy dataset from AIMD snapshots (~5000 frames). Validate RMS force error < 50 meV/Å. Use for ns-scale MD beyond DFT reach."),
    )?;

    db.create_issue(
        "Validate elastic constants vs DFT",
        3.0,
        "analysis",
        &Status::Todo,
        Some(today + Duration::days(14)),
        Some("Use IBRION=6 stress-strain approach in VASP. Extract C11, C12, C44 for Ti3C2. Compare with literature DFT-PBE values within 5%."),
    )?;

    db.create_issue(
        "Literature review: MXene oxidation",
        2.0,
        "lit",
        &Status::Todo,
        None,
        Some("Summarise key papers on MXene surface oxidation in air and water. Focus on Ti3C2 and V2C. Identify activation barriers and temperature dependence. Build annotated bibliography."),
    )?;

    Ok(())
}
