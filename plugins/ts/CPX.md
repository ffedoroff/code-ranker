# CPX — Reduce Complexity

These modules carry the highest cognitive complexity in the project: deeply
nested, branch-heavy logic that is hard for a human to follow and easy to break.

For each module below:

- Extract nested blocks into small, named helper functions.
- Use early returns / guard clauses to flatten nesting.
- Replace sprawling conditionals with a lookup table or polymorphism.
- Split a function that mixes several concerns into focused units.

Keep behaviour identical; the goal is readability, not new features.
