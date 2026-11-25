# T3 native seed brief — French (fr)

Task: T-154 (T3 native-author seed decks). Doctrine: specs/design/2026-10-15-cultural-localization.md §T3 —
authored natively per locale from this brief, NOT translated from the en deck. he decks are the reference
implementation for structure only. Status: model-drafted by the content-author agent following this brief;
pending native-speaker review before classroom use (recorded here, not on the storefront).

## Curriculum anchor

Programmes du collège (cycle 3/4, éduscol consolidés 2020) for math/science/history; English as
foreign language for english-vocabulary. Anchors live in content/curriculum/sources/fr/ (alignment
references only — card text is paraphrased, never reproduced). history-dates-figures-fr is ALREADY
native (105 cards, landed earlier) — left untouched by this task.

## Locale conventions

- Match the in-repo fr precedent (history-dates-figures-fr): ASCII apostrophes (l'humanité), regular
  space before : and ;, «» guillemets for cited terms. Decimal comma (0,25).
- concept_id slugs ASCII-deaccented (equation, mediatrice, element-chimique); card text carries
  proper accents (é è à ç).
- History concept_ids follow the fr deck pattern: y<year>-<slug> for dated events/figures,
  c-<era>-<slug> for era-level concepts.

## Decks (3 authored × 105 cards, grades 7-9 band, units 01-08; history already native)

### math-definitions-fr (47 basic / 47 cloze / 11 type_in)
Keeps all 20 stub concepts (nombre-decimal … factorisation) with rewritten self-contained text; adds
~85 native collège concepts. Unit map: 01 nombres et opérations (nombre premier, multiple, diviseur,
PGCD/PPCM); 02 fractions et décimaux; 03 puissances et racines (racine carrée, puissances); 04
proportionnalité et pourcentages; 05 expressions littérales et équations; 06 fonctions (linéaire,
affine, coefficient directeur); 07 géométrie (angles, triangles, théorèmes de Pythagore et Thalès,
cercle, symétrie); 08 statistiques et probabilités (médiane, étendue, probabilité). type_in stems:
«Écrivez le terme : …» (definition-only, answer = the French term).

### science-terms-fr (63 basic / 32 cloze / 10 type_in)
Keeps all 20 stub concepts (matiere … energie-cinetique); adds ~85. Unit map: 01 matière et états; 02
atomes et molécules; 03 cellule et organismes; 04 nutrition (photosynthèse, respiration); 05 génétique
(ADN, gène, chromosome, hérédité); 06 écosystèmes et biodiversité; 07 forces et mouvement; 08 énergie
et ses formes; 09 chimie quotidienne (réactions, pH); 10 Terre et univers (plaques tectoniques,
gravitation, lumière). Native anchors: cellule, photosynthèse, ADN, écosystème, force, vitesse, pH,
réfraction.

### english-vocabulary-fr (47 basic / 32 cloze / 26 type_in)
EFL deck for French speakers: English word → French gloss. Keeps the 20 stub words (school,
teacher, homework … understand) with rewritten cards; adds ~85 school/daily-life words. Patterns
(from he reference): basic = front English word, back «gloss + English example sentence», extra
French usage note; cloze = front «gloss + English sentence with {{c1::word}}», back = word, extra
French note; type_in = front «Écrivez le mot anglais : …» (French cue), back = English word. No
direction column (build_tags allows only en-he/he-en). Glossary-key words (teacher, student, class*,
question, practice…) use the glossary.csv fr target inside the card text so the glossary alignment
check passes.

## Quality bars applied

BAR 1 (specs/qa/bar1-rules.yaml) + compiler limits (content/seed/generate_decks.py): cloze rendered
text ≤120 chars (hard), type_in front ≤120 (hard), basic front ≤140 / back ≤320 / back ≥20 (warn
bounds), one {{c1:: per cloze, no markup/IPA slashes, no cross-card references, fronts unique,
difficulty=core uniform (matches reference; spread warn is baseline), tags subject::/grade::/unit::
locale-invariant. Census asserted post-compile against the manifest.
