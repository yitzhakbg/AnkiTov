# T3 native seed brief — Spanish (es)

Task: T-154 (T3 native-author seed decks). Doctrine: specs/design/2026-10-15-cultural-localization.md §T3 —
authored natively per locale from this brief, NOT translated from the en deck. he decks are the reference
implementation for structure only. Status: model-drafted by the content-author agent following this brief;
pending native-speaker review before classroom use (recorded here, not on the storefront).

## Curriculum anchor

Spain ESO (LOMLOE RD 217/2022) + primaria bridge (RD 157/2022) for math/science/history; English as
foreign language for english-vocabulary. Anchors live in content/curriculum/sources/es/ (alignment
references only — card text is paraphrased, never reproduced).

## Locale conventions

- Decimal comma (0,25), comma thousands where needed. ¿…? ¡…! pairs. «» guillemets for cited terms.
- ASCII apostrophes are irrelevant (no elisions); straight quotes avoided in prose.
- concept_id slugs ASCII-deaccented (numero-entero, angulos-…); card text carries proper accents.
- History concept_ids follow the es stub pattern: y<year>-<slug> for dated events/figures,
  c-<era>-<slug> for era-level concepts.

## Decks (4 × 105 cards, grades 7-9 band, units 01-08)

### math-definitions-es (47 basic / 47 cloze / 11 type_in)
Keeps all 20 stub concepts (numero-entero … regla-de-laplace) with rewritten self-contained text;
adds ~85 native concepts. Unit map: 01 números y operaciones; 02 fracciones y decimales; 03 potencias
y raíces; 04 proporcionalidad y porcentajes; 05 expresión algebraica y ecuaciones; 06 funciones; 07
geometría (ángulos, polígonos, teoremas); 08 estadística y probabilidad. Native anchors: número primo,
mínimo común múltiplo, notación científica, teorema de Pitágoras, teorema de Tales, media/mediana/moda,
espacio muestral, regla de Laplace. type_in stems: «Escribe el término matemático: …» (definition-only,
answer = the accented term).

### science-terms-es (63 basic / 32 cloze / 10 type_in)
Keeps all 20 stub concepts (materia … placa-tectonica); adds ~85. Unit map: 01 materia y estados; 02
átomos y enlaces; 03 célula y tejidos; 04 nutrición celular (fotosíntesis, respiración); 05 genética
(ADN, genes, herencia); 06 ecosistemas y medio ambiente; 07 fuerzas y movimiento; 08 energía y sus
formas; 09 química de lo cotidiano (pH, reacciones); 10 Tierra y universo (placas, gravedad). Native
anchors: célula, fotosíntesis, ADN, ecosistema, fuerza, velocidad, pH, refracción.

### history-dates-figures-es (73 basic / 32 cloze / 0 type_in)
Keeps all 20 stub events (c-3200-a-c-… → y2001-…); adds ~85 events + figures with Hispanic-world
anchors alongside the western canon: Reconquista (1492 Granada), Colón, Independencias
hispanoamericanas (1810-1824 Bolívar/San Martín kept), Guerra Civil española (1936-1939), Transición
(1978 Constitución). Cloze cards delete the date/figure; basic cards ask «¿Qué significa «X»?» per the
locale qfront stem or natural ¿Quién fue…?/¿Qué fue…? stems. No type_in (locale date formats).

### english-vocabulary-es (47 basic / 32 cloze / 26 type_in)
EFL deck for Spanish speakers: English word → Spanish gloss. Keeps the 20 stub school-domain words;
adds ~85 school/daily-life words. Patterns (from he reference): basic = front English word, back
«gloss + English example sentence», extra Spanish usage note; cloze = front «gloss + English sentence
with {{c1::word}}», back = word, extra Spanish note; type_in = front «Escribe la palabra inglesa: …»
(Spanish cue), back = English word. No direction column (build_tags allows only en-he/he-en).
Glossary-key words (teacher, student, class*, question, practice…) use the glossary.csv es target
(docente, estudiante, clase…) inside the card text so the glossary alignment check passes.

## Quality bars applied

BAR 1 (specs/qa/bar1-rules.yaml) + compiler limits (content/seed/generate_decks.py): cloze rendered
text ≤120 chars (hard), type_in front ≤120 (hard), basic front ≤140 / back ≤320 / back ≥20 (warn
bounds), one {{c1:: per cloze, no markup/IPA slashes, no cross-card references, fronts unique,
difficulty=core uniform (matches reference; spread warn is baseline), tags subject::/grade::/unit::
locale-invariant. Census asserted post-compile against the manifest.
