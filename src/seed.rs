use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime};
use rusqlite::params;

use crate::db::Db;
use crate::models::Status;

/// Fills a database with six weeks of a computational materials postdoc's work
/// (DFT data, machine-learned interatomic potentials, writing, mentoring), relative
/// to today: five finished weekly sprints and the current one.
///
/// The history is realistic on purpose: scope added mid-sprint, an issue taken back
/// out, work carried from week to week, and one fellowship statement that has been
/// put off every single week (the frog).
pub fn seed(db: &Db) -> Result<()> {
    let s = Seeder::new(db);

    // ── Sprints: Monday–Friday, five past weeks and this one ──────────────────
    let w5 = s.sprint(-5)?;
    let w4 = s.sprint(-4)?;
    let w3 = s.sprint(-3)?;
    let w2 = s.sprint(-2)?;
    let w1 = s.sprint(-1)?;
    let w0 = s.sprint(0)?;

    // The one that keeps slipping.
    let fellowship = s.issue(
        "Fellowship proposal: research statement", 5.0, "proposal", s.t(-5, 0, 9),
        "Two pages: vision, three aims, why this host group. Reuse the MLIP + active-learning story, but lead with a hypothesis, not a list of methods.",
    )?;
    s.join(fellowship, w5, s.t(-5, 0, 10))?;
    for (week, sprint) in [(-4, w4), (-3, w3), (-2, w2), (-1, w1), (0, w0)] {
        s.carry(fellowship, sprint, week)?;
    }
    s.due(fellowship, 9)?;

    // ── Week -5: set-up ───────────────────────────────────────────────────────
    let conv = s.issue(
        "ENCUT and k-point convergence (Li3PS4)", 2.0, "dft-dataset", s.t(-6, 3, 10),
        "PBEsol, PAW. Converge total energies to 1 meV/atom and forces to 5 meV/Å before generating any training data.",
    )?;
    s.join(conv, w5, s.t(-5, 0, 10))?;
    s.done(conv, s.t(-5, 0, 11), s.t(-5, 1, 16))?;

    let atomate = s.issue(
        "atomate2 workflows on the cluster", 3.0, "hpc", s.t(-5, 0, 9),
        "Relax + static with a custom INCAR, results to the database. One job array per batch.",
    )?;
    s.join(atomate, w5, s.t(-5, 0, 10))?;
    s.done(atomate, s.t(-5, 1, 9), s.t(-5, 3, 17))?;
    s.subtask(atomate, "Configure jobflow-remote", s.t(-5, 1, 9), Some((s.t(-5, 1, 9), Some(s.t(-5, 2, 12)))))?;
    s.subtask(atomate, "Test one relaxation end to end", s.t(-5, 1, 9), Some((s.t(-5, 2, 13), Some(s.t(-5, 3, 17)))))?;

    let reading = s.issue(
        "Read up on fine-tuning foundation MLIPs", 1.0, "reading", s.t(-5, 0, 9),
        "What to freeze, how much data is enough, and how others report errors.",
    )?;
    s.join(reading, w5, s.t(-5, 0, 10))?;
    s.done(reading, s.t(-5, 3, 9), s.t(-5, 4, 15))?;

    // ── Week -4: training data ────────────────────────────────────────────────
    let rattle = s.issue(
        "Rattled and strained structures (300)", 2.0, "dft-dataset", s.t(-5, 4, 10),
        "±3% strain, 0.05 Å rattles, plus Li vacancies. Keep the generating script in the repo.",
    )?;
    s.join(rattle, w4, s.t(-4, 0, 10))?;
    s.done(rattle, s.t(-4, 0, 11), s.t(-4, 1, 15))?;

    let batch1 = s.issue(
        "VASP single points, training batch 1", 3.0, "dft-dataset", s.t(-4, 0, 9),
        "300 single points. Check SCF convergence, drop outliers, export to extxyz.",
    )?;
    s.join(batch1, w4, s.t(-4, 0, 10))?;
    s.done(batch1, s.t(-4, 1, 9), s.t(-4, 3, 18))?;

    let port = s.issue(
        "Port analysis scripts to the new pymatgen API", 2.0, "hpc", s.t(-4, 0, 9),
        "Deprecation warnings everywhere. Not urgent until something actually breaks.",
    )?;
    s.join(port, w4, s.t(-4, 0, 10))?;
    s.remove(port, w4, s.t(-4, 2, 14))?;

    let slurm = s.issue(
        "Fix SLURM array job memory limit", 1.0, "hpc", s.t(-4, 2, 10),
        "OOM kills on the larger supercells; set --mem-per-cpu per job size.",
    )?;
    s.join(slurm, w4, s.t(-4, 2, 10))?;
    s.done(slurm, s.t(-4, 2, 10), s.t(-4, 2, 16))?;

    // ── Week -3: first potential ──────────────────────────────────────────────
    let finetune = s.issue(
        "Fine-tune MACE on batch 1", 3.0, "mlip", s.t(-4, 3, 10),
        "Start from the medium foundation model. 80/10/10 split, early stopping on force MAE.",
    )?;
    s.join(finetune, w3, s.t(-3, 0, 10))?;
    s.done(finetune, s.t(-3, 0, 11), s.t(-3, 2, 17))?;

    let parity = s.issue(
        "Parity plots: energies, forces, stresses", 2.0, "mlip", s.t(-3, 0, 9),
        "Test set only. Report MAE and RMSE per species for forces.",
    )?;
    s.join(parity, w3, s.t(-3, 0, 10))?;
    s.done(parity, s.t(-3, 2, 9), s.t(-3, 3, 15))?;

    let referee = s.issue(
        "Referee report for a journal", 2.0, "reviews", s.t(-3, 0, 9),
        "Manuscript on ML potentials for oxide interfaces. Check the validation, not just the parity plots.",
    )?;
    s.join(referee, w3, s.t(-3, 0, 10))?;
    s.carry(referee, w2, -2)?;
    s.done(referee, s.t(-2, 1, 9), s.t(-2, 2, 12))?;

    let outline = s.issue(
        "MSc student: thesis outline feedback", 1.0, "mentoring", s.t(-3, 0, 9),
        "",
    )?;
    s.join(outline, w3, s.t(-3, 0, 10))?;
    s.done(outline, s.t(-3, 1, 14), s.t(-3, 1, 16))?;

    // ── Week -2: active learning ──────────────────────────────────────────────
    let committee = s.issue(
        "Active learning: committee disagreement on MD frames", 3.0, "active-learn", s.t(-3, 3, 9),
        "Five-model committee, pick frames by force disagreement, cap at 150 per round.",
    )?;
    s.join(committee, w2, s.t(-2, 0, 10))?;
    s.done(committee, s.t(-2, 0, 11), s.t(-2, 4, 16))?;

    let talk = s.issue(
        "Group meeting talk: MLIP progress", 2.0, "talks", s.t(-3, 4, 10),
        "20 minutes. Errors vs. data size, first diffusion numbers, open questions.",
    )?;
    s.join(talk, w2, s.t(-2, 0, 10))?;
    s.done(talk, s.t(-2, 2, 9), s.t(-2, 3, 11))?;

    let methods = s.issue(
        "Paper: methods section draft", 3.0, "paper", s.t(-2, 0, 9),
        "DFT settings, dataset construction, training protocol, active-learning loop. Enough detail to reproduce.",
    )?;
    s.join(methods, w2, s.t(-2, 0, 10))?;
    s.start(methods, s.t(-2, 3, 10))?;
    s.carry(methods, w1, -1)?;
    s.done(methods, s.t(-2, 3, 10), s.t(-1, 2, 16))?;

    // ── Week -1: second round ─────────────────────────────────────────────────
    let batch2 = s.issue(
        "DFT single points for 120 selected frames", 3.0, "dft-dataset", s.t(-2, 4, 15),
        "Same settings as batch 1, so the two batches can be merged.",
    )?;
    s.join(batch2, w1, s.t(-1, 0, 10))?;
    s.done(batch2, s.t(-1, 0, 11), s.t(-1, 3, 17))?;

    let retrain = s.issue(
        "Retrain MACE with active-learning round 1", 3.0, "mlip", s.t(-1, 0, 9),
        "Compare against the batch-1 model on the same test set.",
    )?;
    s.join(retrain, w1, s.t(-1, 0, 10))?;
    s.done(retrain, s.t(-1, 3, 18), s.t(-1, 4, 15))?;

    let diffusion = s.issue(
        "Li diffusion: MD at 300–700 K with fine-tuned MACE", 5.0, "mlip", s.t(-1, 0, 9),
        "NVT, 2 ns per temperature, 4×4×4 supercell. MSD → D(T) → Arrhenius fit for the activation energy.",
    )?;
    s.join(diffusion, w1, s.t(-1, 0, 10))?;
    s.start(diffusion, s.t(-1, 4, 9))?;
    s.carry(diffusion, w0, 0)?;
    s.due(diffusion, 3)?;

    let phonopy = s.issue(
        "Help the student debug a phonopy run", 1.0, "mentoring", s.t(-1, 2, 14),
        "Imaginary modes at Γ: supercell too small, or forces not converged?",
    )?;
    s.join(phonopy, w1, s.t(-1, 2, 14))?;
    s.done(phonopy, s.t(-1, 2, 14), s.t(-1, 2, 17))?;

    // ── Week 0: this sprint ───────────────────────────────────────────────────
    let aimd = s.issue(
        "Compare Li diffusivity with an AIMD reference", 3.0, "mlip", s.t(-1, 4, 10),
        "Same cell, 50 ps AIMD at 700 K as the reference point.",
    )?;
    s.join(aimd, w0, s.t(0, 0, 10))?;
    s.due(aimd, 2)?;

    let results = s.issue(
        "Paper: results section, figures 2–4", 3.0, "paper", s.t(-1, 3, 10),
        "One message per figure. Shared colors for temperatures across panels.",
    )?;
    s.join(results, w0, s.t(0, 0, 10))?;
    s.start(results, s.t(0, 0, 11))?;
    s.subtask(results, "Fig. 2: parity plots", s.t(0, 0, 11), Some((s.t(0, 0, 11), Some(s.t(0, 0, 16)))))?;
    s.subtask(results, "Fig. 3: Arrhenius plot", s.t(0, 0, 11), Some((s.t(0, 1, 10), None)))?;
    s.subtask(results, "Fig. 4: Li-Li radial distribution", s.t(0, 0, 11), None)?;
    s.due(results, 6)?;

    let compute = s.issue(
        "Compute time renewal request", 2.0, "proposal", s.t(-2, 1, 9),
        "Node-hours from last year's usage plus the MD campaign; two pages.",
    )?;
    s.join(compute, w0, s.t(0, 0, 10))?;
    s.done(compute, s.t(0, 0, 11), s.t(0, 1, 12))?;

    let coauthor = s.issue(
        "Reply to co-author comments on the SI", 1.0, "paper", s.t(0, 0, 9),
        "",
    )?;
    s.join(coauthor, w0, s.t(0, 0, 10))?;
    s.done(coauthor, s.t(0, 0, 14), s.t(0, 0, 15))?;

    let checkin = s.issue("MSc student: weekly check-in", 1.0, "mentoring", s.t(0, 0, 9), "")?;
    s.join(checkin, w0, s.t(0, 0, 10))?;
    s.due(checkin, 1)?;

    let conda = s.issue(
        "Update conda env (torch, cuequivariance)", 1.0, "hpc", s.t(0, 1, 9),
        "Pin versions; the GPU nodes got new drivers.",
    )?;
    s.join(conda, w0, s.t(0, 1, 9))?;
    s.done(conda, s.t(0, 1, 9), s.t(0, 1, 11))?;

    let abstract_ = s.issue(
        "Abstract for the spring meeting", 2.0, "talks", s.t(0, 1, 14),
        "300 words. Diffusion results only if the AIMD comparison holds up.",
    )?;
    s.join(abstract_, w0, s.t(0, 1, 14))?;
    s.due(abstract_, 5)?;

    // ── Backlog ───────────────────────────────────────────────────────────────
    let vasp = s.issue("Install VASP 6 on the GPU partition", 2.0, "hpc", s.t(-6, 1, 9), "")?;
    s.done(vasp, s.t(-6, 1, 10), s.t(-6, 2, 17))?;
    s.issue(
        "Surface slabs for interface stability", 5.0, "dft-dataset", s.t(-4, 4, 10),
        "Li metal | Li3PS4 interface: three terminations, six-layer slabs, dipole correction.",
    )?;
    let lecture = s.issue(
        "Lecture: DFT for the materials course", 3.0, "teaching", s.t(-3, 1, 10),
        "90 minutes, from Kohn-Sham to what the INCAR tags mean. One live demo.",
    )?;
    s.due(lecture, 20)?;
    s.issue(
        "Benchmark MACE, NequIP and CHGNet on the test set", 5.0, "mlip", s.t(-3, 2, 10),
        "Same splits and metrics for all three; include inference cost per atom-step.",
    )?;
    s.issue(
        "Calibrate committee uncertainty", 3.0, "active-learn", s.t(-2, 4, 11),
        "Is force disagreement a usable error estimate? Plot it against the true error.",
    )?;
    s.issue("README for the active-learning repo", 2.0, "hpc", s.t(-1, 1, 10), "")?;

    // Newest first in the backlog, like issues created by hand.
    db.rerank_by_created_at()?;
    // Past sprints' finished work stays out of the backlog list (c shows it).
    db.set_setting("show_completed", "false")?;
    Ok(())
}

struct Seeder<'a> {
    db: &'a Db,
    now: NaiveDateTime,
    /// Monday of the current week.
    monday: NaiveDate,
}

impl<'a> Seeder<'a> {
    fn new(db: &'a Db) -> Self {
        let now = Local::now().naive_local();
        let today = now.date();
        let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
        Seeder { db, now, monday }
    }

    /// `hour` o'clock on weekday `day` (0 = Monday) of the week `week` weeks from this
    /// one (negative = past). Never later than now, so this week's events stay in the past.
    fn t(&self, week: i64, day: i64, hour: u32) -> NaiveDateTime {
        let date = self.monday + Duration::days(7 * week + day);
        date.and_hms_opt(hour, 0, 0).unwrap_or(self.now).min(self.now)
    }

    fn ts(t: NaiveDateTime) -> String {
        t.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    /// A Monday–Friday sprint, planned on its Monday morning; week 0 is the active one.
    fn sprint(&self, week: i64) -> Result<i64> {
        let start = self.monday + Duration::days(7 * week);
        let name = format!("Week {}", start.iso_week().week());
        let id = self.db.create_sprint(&name, start, start + Duration::days(4), week == 0)?;
        self.db.conn().execute(
            "UPDATE sprints SET created_at = ?1 WHERE id = ?2",
            params![Self::ts(self.t(week, 0, 9)), id],
        )?;
        Ok(id)
    }

    fn issue(&self, title: &str, sp: f64, epic: &str, created: NaiveDateTime, desc: &str) -> Result<i64> {
        let c = Self::ts(created);
        let desc = Some(desc).filter(|d| !d.is_empty());
        self.db.create_issue_full(title, sp, epic, &Status::Todo, None, desc, None, &c, &c, None)
    }

    fn due(&self, id: i64, days_from_today: i64) -> Result<()> {
        self.db.set_issue_due_date(id, self.now.date() + Duration::days(days_from_today))
    }

    fn start(&self, id: i64, at: NaiveDateTime) -> Result<()> {
        self.db.conn().execute(
            "UPDATE issues SET status = 'IN_PROGRESS', started_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![Self::ts(at), id],
        )?;
        Ok(())
    }

    fn done(&self, id: i64, started: NaiveDateTime, finished: NaiveDateTime) -> Result<()> {
        self.db.conn().execute(
            "UPDATE issues SET status = 'DONE', started_at = ?1, completed_at = ?2, updated_at = ?2
             WHERE id = ?3",
            params![Self::ts(started), Self::ts(finished), id],
        )?;
        Ok(())
    }

    /// Put an issue into a sprint at `at` (after the sprint's Monday = unplanned scope).
    fn join(&self, id: i64, sprint: i64, at: NaiveDateTime) -> Result<()> {
        self.db.conn().execute(
            "INSERT INTO issue_sprint_log (issue_id, sprint_id, added_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(issue_id, sprint_id) DO UPDATE SET removed_at = NULL",
            params![id, sprint, Self::ts(at)],
        )?;
        self.db.conn().execute("UPDATE issues SET sprint_id = ?1 WHERE id = ?2", params![sprint, id])?;
        Ok(())
    }

    /// Carry an unfinished issue into the sprint of week `week`, the way starting that
    /// sprint does: it joins at the start and gets a carry-over mark.
    fn carry(&self, id: i64, sprint: i64, week: i64) -> Result<()> {
        self.join(id, sprint, self.t(week, 0, 0))?;
        self.db.conn().execute(
            "UPDATE issues SET carry_count = carry_count + 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Take an issue back out of a running sprint.
    fn remove(&self, id: i64, sprint: i64, at: NaiveDateTime) -> Result<()> {
        self.db.conn().execute(
            "UPDATE issue_sprint_log SET removed_at = ?1 WHERE issue_id = ?2 AND sprint_id = ?3",
            params![Self::ts(at), id, sprint],
        )?;
        self.db.conn().execute("UPDATE issues SET sprint_id = NULL WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// A subtask; `progress` is (started, finished) for work that has begun.
    fn subtask(
        &self,
        parent: i64,
        title: &str,
        created: NaiveDateTime,
        progress: Option<(NaiveDateTime, Option<NaiveDateTime>)>,
    ) -> Result<i64> {
        let c = Self::ts(created);
        let id = self.db.create_issue_full(title, 0.0, "", &Status::Todo, None, None, Some(parent), &c, &c, None)?;
        match progress {
            Some((started, Some(finished))) => self.done(id, started, finished)?,
            Some((started, None)) => self.start(id, started)?,
            None => {}
        }
        Ok(id)
    }
}
