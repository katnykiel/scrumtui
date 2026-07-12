use anyhow::Result;
use chrono::{Duration, Local};

use crate::db::Db;
use crate::models::Status;

/// Populate a temporary database with sample postdoc research data.
///
/// Timeline (all dates relative to today):
///   Sprint 1  — 5 weeks ago (completed)
///   Sprint 2  — 4 weeks ago (completed, 2 issues carried to sprint 3)
///   Sprint 3  — 3 weeks ago (completed, 1 issue carried to sprint 4)
///   Sprint 4  — 2 weeks ago (completed, 1 issue carried to sprint 5)
///   Sprint 5  — current, started 3 days ago
///
/// Epics are staggered so the Gantt view shows distinct lanes:
///   mxene-ox   started ~5 weeks ago
///   he-carb    started ~4 weeks ago
///   nequip     started ~3 weeks ago
///   writing    started ~2 weeks ago
pub fn seed(db: &Db) -> Result<()> {
    let today = Local::now().date_naive();

    // ── helpers ───────────────────────────────────────────────────────────────
    let dt = |days_ago: i64| -> String {
        (today - Duration::days(days_ago))
            .format("%Y-%m-%d 09:00:00")
            .to_string()
    };
    let dt_done = |days_ago: i64| -> String {
        (today - Duration::days(days_ago))
            .format("%Y-%m-%d 17:00:00")
            .to_string()
    };
    let date = |days: i64| today + Duration::days(days);

    // ── sprints ───────────────────────────────────────────────────────────────
    let s1 = db.create_sprint(
        "Sprint 1 — MXene setup",
        today - Duration::days(35),
        today - Duration::days(29),
        false,
    )?;
    let s2 = db.create_sprint(
        "Sprint 2 — Oxidation calcs",
        today - Duration::days(28),
        today - Duration::days(22),
        false,
    )?;
    let s3 = db.create_sprint(
        "Sprint 3 — HE-carbide + AIMD",
        today - Duration::days(21),
        today - Duration::days(15),
        false,
    )?;
    let s4 = db.create_sprint(
        "Sprint 4 — NequIP + writing",
        today - Duration::days(14),
        today - Duration::days(8),
        false,
    )?;
    let s5 = db.create_sprint(
        "Sprint 5 — Revisions",
        today - Duration::days(3),
        today + Duration::days(3),
        true,
    )?;

    // ── macro for issue creation ───────────────────────────────────────────────
    // Issues in sprint 1 (mxene-ox epic, all done)
    let i1 = db.create_issue_full(
        "Ti3C2 slab supercell setup",
        2.0, "mxene-ox", &Status::Done, None,
        Some("3-layer Ti3C2 slab, 4×4×1 supercell, 15 Å vacuum. Compare PBE-D3 vs vdW-DF2 interlayer spacing against XRD."),
        None, &dt(34), &dt_done(32), Some(&dt_done(32)),
    )?;
    db.set_issue_sprint(i1, Some(s1))?;

    let i2 = db.create_issue_full(
        "ENCUT/k-mesh convergence",
        2.0, "mxene-ox", &Status::Done, None,
        Some("Sweep ENCUT 400–700 eV and k-mesh 6×6×1 to 12×12×1. Target <1 meV/atom total energy convergence."),
        None, &dt(33), &dt_done(31), Some(&dt_done(31)),
    )?;
    db.set_issue_sprint(i2, Some(s1))?;

    let i3 = db.create_issue_full(
        "MXene oxidation lit review",
        2.0, "mxene-ox", &Status::Done, None,
        Some("Survey Ti3C2 and V2C oxidation papers 2018–2025. Extract activation barriers, temperature windows, termination effects. Build annotated Zotero collection."),
        None, &dt(35), &dt_done(30), Some(&dt_done(30)),
    )?;
    db.set_issue_sprint(i3, Some(s1))?;

    // Sprint 2: MXene oxidation calculations; HE-carbide started; 2 issues carry to s3
    let i4 = db.create_issue_full(
        "O/OH/F termination relaxations",
        3.0, "mxene-ox", &Status::Done, None,
        Some("ISIF=2 relaxation for all three termination types. Compare adsorption energies and surface charge density plots."),
        None, &dt(27), &dt_done(25), Some(&dt_done(25)),
    )?;
    db.set_issue_sprint(i4, Some(s2))?;

    let i5 = db.create_issue_full(
        "Surface energy vs O coverage",
        3.0, "mxene-ox", &Status::Done, None,
        Some("Compute E_surf for 0–100% O coverage in 25% steps. Identify thermodynamic crossover coverage. Plot convex hull."),
        None, &dt(27), &dt_done(24), Some(&dt_done(24)),
    )?;
    db.set_issue_sprint(i5, Some(s2))?;

    // This one starts in s2 but gets carried to s3
    let i6 = db.create_issue_full(
        "HEA-carbide structure search",
        3.0, "he-carb", &Status::Done, None,
        Some("Generate (Ti,Zr,Hf,V,Nb)C SQS supercells via ATAT. Screen 20 configs for lowest DFT total energy. Keep 5 for full relaxation."),
        None, &dt(28), &dt_done(16), Some(&dt_done(16)),
    )?;
    db.set_issue_sprint(i6, Some(s2))?;
    db.set_issue_sprint(i6, Some(s3))?; // carried to s3, completed there

    // Starts in s2, carries all the way to s4
    let i7 = db.create_issue_full(
        "O-termination SI writeup",
        3.0, "writing", &Status::Done, None,
        Some("Supporting information: convergence tables, all termination geometry plots, charge density difference maps. ~8 pages."),
        None, &dt(26), &dt_done(10), Some(&dt_done(10)),
    )?;
    db.set_issue_sprint(i7, Some(s2))?;
    db.set_issue_sprint(i7, Some(s3))?; // carried
    db.set_issue_sprint(i7, Some(s4))?; // carried again, finally done

    // Sprint 3: AIMD + HEA-carbide deep dive
    let i8 = db.create_issue_full(
        "AIMD 300 K Ti3C2 10 ps",
        5.0, "mxene-ox", &Status::Done, None,
        Some("NVT Nosé-Hoover, POTIM=2 fs, 5000 steps. Equilibration check via T and P fluctuations. Export XDATCAR for MSD analysis."),
        None, &dt(21), &dt_done(17), Some(&dt_done(17)),
    )?;
    db.set_issue_sprint(i8, Some(s3))?;

    let i9 = db.create_issue_full(
        "MSD and diffusion coefficient",
        2.0, "mxene-ox", &Status::Done, None,
        Some("Parse XDATCAR with pyiron. Plot mean squared displacement vs time. Extract D via Einstein relation for Ti and C sublattices separately."),
        None, &dt(19), &dt_done(16), Some(&dt_done(16)),
    )?;
    db.set_issue_sprint(i9, Some(s3))?;

    let i10 = db.create_issue_full(
        "HEA-C elastic constants",
        3.0, "he-carb", &Status::Done, None,
        Some("IBRION=6 stress-strain in VASP for top-3 SQS configs. Extract C11, C12, C44 tensor. Compare with rule-of-mixtures prediction."),
        None, &dt(20), &dt_done(9), Some(&dt_done(9)),
    )?;
    db.set_issue_sprint(i10, Some(s3))?;
    db.set_issue_sprint(i10, Some(s4))?; // carried to s4, done there

    let i11 = db.create_issue_full(
        "NequIP AIMD dataset prep",
        2.0, "nequip", &Status::Done, None,
        Some("Extract 5000 frames from Ti3C2 AIMD trajectory. Run single-point VASP for forces and energies. Export to extxyz for NequIP training."),
        None, &dt(21), &dt_done(18), Some(&dt_done(18)),
    )?;
    db.set_issue_sprint(i11, Some(s3))?;

    // Sprint 4: NequIP training + paper draft
    let i12 = db.create_issue_full(
        "NequIP force error < 50 meV/Å",
        4.0, "nequip", &Status::Done, None,
        Some("Train NequIP with l_max=2, num_layers=4. Validate on 500-frame holdout. Iterate on cutoff radius (4–6 Å) until RMS force < 50 meV/Å."),
        None, &dt(14), &dt_done(10), Some(&dt_done(10)),
    )?;
    db.set_issue_sprint(i12, Some(s4))?;

    let i13 = db.create_issue_full(
        "MXene paper: intro draft",
        3.0, "writing", &Status::Done, None,
        Some("~1500 words. Hook: MXene stability limits device lifetimes. Literature gap: atomistic oxidation mechanism unknown. Scope: DFT + AIMD + ML-MD."),
        None, &dt(14), &dt_done(11), Some(&dt_done(11)),
    )?;
    db.set_issue_sprint(i13, Some(s4))?;

    let i14 = db.create_issue_full(
        "MXene paper: methods draft",
        2.0, "writing", &Status::Done, None,
        Some("DFT setup, pseudopotentials, k-mesh, AIMD protocol, NequIP architecture, validation metrics. Aim for reproducibility."),
        None, &dt(13), &dt_done(8), Some(&dt_done(8)),
    )?;
    db.set_issue_sprint(i14, Some(s4))?;

    // ── Active sprint (s5) ────────────────────────────────────────────────────
    // Done this week
    let i15 = db.create_issue_full(
        "HSE06 band gap spot-check",
        3.0, "mxene-ox", &Status::Done, None,
        Some("Two representative terminations (O and OH). Compare HSE06 vs PBE band edges at Γ. Confirm metallic/semiconducting character for SI table."),
        None, &dt(3), &dt_done(2), Some(&dt_done(2)),
    )?;
    db.set_issue_sprint(i15, Some(s5))?;

    let i16 = db.create_issue_full(
        "Revise introduction",
        2.0, "writing", &Status::Done, None,
        Some("Incorporate reviewer 2 comments: sharpen gap statement, add 2 recent MXene stability refs (Gogotsi 2024, Anasori 2025)."),
        None, &dt(3), &dt_done(1), Some(&dt_done(1)),
    )?;
    db.set_issue_sprint(i16, Some(s5))?;

    // In progress
    let i17 = db.create_issue_full(
        "HEA-C phonon stability",
        4.0, "he-carb", &Status::InProgress,
        Some(date(3)), // due end of sprint
        Some("Finite-displacement phonon calculation (phonopy) for lowest-energy SQS. Check for imaginary modes at Γ and X. 3×3×3 supercell."),
        None, &dt(2), &dt(2), None,
    )?;
    db.set_issue_sprint(i17, Some(s5))?;

    let i18 = db.create_issue_full(
        "NequIP 1 ns MD run",
        5.0, "nequip", &Status::InProgress,
        Some(date(3)),
        Some("Deploy trained potential for 1 ns NVT at 300, 600, 900 K. Monitor structural stability and O diffusion coefficient. Compare with DFT AIMD reference."),
        None, &dt(2), &dt(2), None,
    )?;
    db.set_issue_sprint(i18, Some(s5))?;

    // Todo this sprint
    let i19 = db.create_issue_full(
        "Results figures (main text)",
        3.0, "writing", &Status::Todo,
        Some(date(2)),
        Some("4 panels: (1) surface energy convex hull, (2) AIMD MSD, (3) NequIP parity plot, (4) ML-MD O diffusion vs T. High-res PDF, consistent colour scheme."),
        None, &dt(1), &dt(1), None,
    )?;
    db.set_issue_sprint(i19, Some(s5))?;

    let i20 = db.create_issue_full(
        "Group meeting talk",
        2.0, "writing", &Status::Todo,
        Some(date(1)),
        Some("30 min + Q&A. Audience: mixed theory/experiment. Cover motivation, key DFT results, AIMD stability, NequIP validation, outlook."),
        None, &dt(1), &dt(1), None,
    )?;
    db.set_issue_sprint(i20, Some(s5))?;

    // ── Backlog ───────────────────────────────────────────────────────────────
    db.create_issue_full(
        "Submit to npj Comp. Mater.",
        3.0, "writing", &Status::Todo,
        Some(date(14)),
        Some("Final manuscript assembly: main text + SI + cover letter. Check journal formatting (4000 word limit main text). Submit via editorial manager."),
        None, &dt(1), &dt(1), None,
    )?;

    db.create_issue_full(
        "Phonon dispersion Ti3C2",
        5.0, "mxene-ox", &Status::Todo,
        Some(date(21)),
        Some("DFPT with VASP, 3×3×1 supercell. Compare acoustic branches at Γ with Raman experiment. Check for soft modes near oxidation threshold coverage."),
        None, &dt(5), &dt(5), None,
    )?;

    db.create_issue_full(
        "HEA-C vacancy formation energy",
        3.0, "he-carb", &Status::Todo,
        None,
        Some("C vacancy in top-3 SQS configs. E_vac = E_defect − E_host + μ_C. Compare with binary TiC, ZrC references. Correlate with elastic softening."),
        None, &dt(10), &dt(10), None,
    )?;

    db.create_issue_full(
        "NequIP thermal conductivity",
        5.0, "nequip", &Status::Todo,
        None,
        Some("Green-Kubo heat flux autocorrelation from 10 ns ML-MD trajectory. Compare PBE phonon lifetimes from DFPT. Target: κ vs T for bare and O-terminated MXene."),
        None, &dt(7), &dt(7), None,
    )?;

    db.create_issue_full(
        "Postdoc fellowship app",
        3.0, "admin", &Status::Todo,
        Some(date(21)),
        Some("NSF MPS-Ascend or DOE SCGSR. Draft 2-page research statement, CV, 3 reference letters. Align with MXene/ML-MD narrative from current paper."),
        None, &dt(4), &dt(4), None,
    )?;

    db.create_issue_full(
        "Collaborate with XPS group",
        2.0, "mxene-ox", &Status::Todo,
        None,
        Some("Share computed Bader charges and partial DOS near Fermi level. Request Ti 2p and C 1s XPS for direct comparison. Plan co-authorship on follow-up letter."),
        None, &dt(3), &dt(3), None,
    )?;

    Ok(())
}
