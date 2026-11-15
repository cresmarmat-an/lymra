/*!
 * `while` loops, worked out to a fixed point at their head: bounds that hold
 * every time the condition is tested.
 *
 * The head starts as the bounds before the loop. Each round adds where one
 * more iteration from the head comes back to it: the end of the body, and
 * every `continue`. After a few rounds, a bound that still moves goes to the
 * end of its type, so the rounds always end. They end when one more iteration
 * stays within the head, which then holds at every test of the condition,
 * however many iterations run.
 *
 * Widening can jump past a bound the condition sets, as in `while divisor *
 * divisor <= candidate`. A few more rounds take tighter bounds back, keeping
 * them only when one more iteration still stays within them.
 *
 * The body is then walked once more from the head, narrowed by the
 * condition, to remove checks. After the loop, the condition was false at
 * the head, or a `break` left.
 */

use super::analysis::{Analyzer, Bounds, Flow, Mode, WIDENING_DELAY, forget, unfollow};
use super::interval::Interval;
use super::loops::{self, Changes};
use crate::ir::{Block, Expr};

/// Rounds that try to take back bounds that widening went past.
const NARROWING_ROUNDS: usize = 2;

impl Analyzer<'_> {
    pub(super) fn walk_while(
        &mut self,
        condition: &mut Expr,
        body: &mut Block,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) {
        let mut changed = loops::changed_outside(body);
        loops::collect_references(condition, &mut changed);
        // A loop being probed around this one cannot add up how this one changes them.
        unfollow(&changed, changes);
        let head = match self.loop_head(condition, body, bounds, mode) {
            Some(head) => head,
            // Out of budget: every variable the loop changes can hold any value.
            None => {
                let mut head = bounds.clone();
                forget(&changed, &mut head, &mut Changes::new());
                head
            }
        };
        // The condition is tested at the head of every iteration, and once more when it ends the loop.
        let mut tested = head;
        self.evaluate(condition, &mut tested, &mut Changes::new(), mode);
        let mut inside = tested.clone();
        self.refine(&mut inside, condition, true);
        let (_, exits) = self.walk_loop_body(body, &mut inside, &mut Changes::new(), mode);
        let mut after = tested;
        self.refine(&mut after, condition, false);
        if let Some(broken) = exits.broken {
            after = after.join(&broken);
        }
        *bounds = after;
    }

    /// Bounds that hold every time the condition is tested, or `None` when the budget runs out first.
    fn loop_head(
        &mut self,
        condition: &mut Expr,
        body: &mut Block,
        entry: &Bounds,
        mode: Mode,
    ) -> Option<Bounds> {
        // A loop probed inside another widens at once, so nested loops stay cheap.
        let delay = if mode == Mode::Remove {
            WIDENING_DELAY
        } else {
            0
        };
        let mut head = entry.clone();
        let mut round = 0;
        let mut widened = false;
        loop {
            let next = self.after_iteration(condition, body, &head, entry)?;
            if next.within(&head) {
                break;
            }
            let joined = head.join(&next);
            head = if round >= delay {
                widened = true;
                self.widen(&head, &joined)
            } else {
                joined
            };
            round += 1;
        }
        // Without widening, the head holds only values some iteration reaches, and nothing can be taken back.
        let narrowing_rounds = if widened { NARROWING_ROUNDS } else { 0 };
        for _ in 0..narrowing_rounds {
            let Some(next) = self.after_iteration(condition, body, &head, entry) else {
                break;
            };
            let narrowed = next.meet(&head);
            if narrowed == head {
                break;
            }
            // Tighter bounds are kept only where one more iteration stays within them.
            match self.after_iteration(condition, body, &narrowed, entry) {
                Some(again) if again.within(&narrowed) => head = narrowed,
                Some(_) | None => break,
            }
        }
        Some(head)
    }

    /**
     * Where the head stands after one more iteration from `head`: the bounds
     * before the loop, joined with every way back to the head. Spends one
     * body walk of the budget.
     */
    fn after_iteration(
        &mut self,
        condition: &mut Expr,
        body: &mut Block,
        head: &Bounds,
        entry: &Bounds,
    ) -> Option<Bounds> {
        self.budget = self.budget.checked_sub(1)?;
        let mut state = head.clone();
        self.evaluate(condition, &mut state, &mut Changes::new(), Mode::Probe);
        self.refine(&mut state, condition, true);
        let (flow, exits) = self.walk_loop_body(body, &mut state, &mut Changes::new(), Mode::Probe);
        let mut back = entry.clone();
        if flow == Flow::Continues {
            back = back.join(&state);
        }
        if let Some(continued) = exits.continued {
            back = back.join(&continued);
        }
        Some(back)
    }

    /// `new`, where a bound that grew past `old` goes to the end of its local's type.
    fn widen(&self, old: &Bounds, new: &Bounds) -> Bounds {
        let mut widened = new.clone();
        for (local, grown) in new.iter() {
            let (Some(grown), Some(before), Some(ty)) =
                (grown, old.get(local), self.integer_type(local))
            else {
                continue;
            };
            let whole = Interval::of_type(ty);
            let bounds = before.widen(grown, whole);
            widened.set(local, (bounds != whole).then_some(bounds));
        }
        widened
    }
}
