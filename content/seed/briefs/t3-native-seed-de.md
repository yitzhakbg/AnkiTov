# T3 native seed brief — German (de)

Task: T-154 (T3 native-author seed decks). Doctrine: specs/design/2026-10-15-cultural-localization.md §T3 —
authored natively per locale from this brief, NOT translated from the en deck. he decks are the reference
implementation for structure only. Status: model-drafted by the content-author agent following this brief;
pending native-speaker review before classroom use (recorded here, not on the storefront).

## Curriculum anchor

KMK Bildungsstandards Sekundarstufe I + Länder Lehrpläne (the same anchor family the Wave-L1
de-core-vocabulary / de-orthography-rules packs use) for math/science/history; English as foreign
language for english-vocabulary. Anchors: content/curriculum/sources/de/ (alignment references only —
card text is paraphrased, never reproduced).

## Locale conventions

- Decimal comma (0,25). ß/ä/ö/ü in card text; concept_id slugs ASCII-transliterated (ae/oe/ue/ss:
  groesste-gemeinsame-teiler, abhaengige-variable).
- Formal Sie-form in type_in stems («Schreiben Sie den Fachbegriff: …»); neutral question forms
  elsewhere (Was ist …?, Wer war …?, Wann …?).
- History concept_ids follow the he/es pattern: y<year>-<slug> for dated events/figures,
  c-<era>-<slug> for era-level concepts.

## Decks (4 × 105 cards, grades 7-9 band, units 01-08) — all NEW (de had no decks in the 4-slug set)

### math-definitions-de (47 basic / 47 cloze / 11 type_in)
Unit map: 01 Zahlen und Operationen (Primzahl, Teilbarkeit, ggt/kgV); 02 Brüche und Dezimalzahlen; 03
Potenzen und Wurzeln (Quadratwurzel, Potenzgesetze); 04 Proportionalität und Prozent (Dreisatz); 05
Terme und Gleichungen (Variable, Äquivalenzumformung); 06 Funktionen (linear, Steigung); 07 Geometrie
(Winkel, Dreiecke, Satz des Pythagoras, Kreis); 08 Statistik und Wahrscheinlichkeit (Mittelwert,
Median, Wahrscheinlichkeit). type_in stems: «Schreiben Sie den Fachbegriff: …» (definition-only,
answer = the German term).

### science-terms-de (63 basic / 32 cloze / 10 type_in)
Unit map: 01 Stoffe und Teilchen (Atom, Molekül, Aggregatzustände); 02 Zelle und Organismen; 03
Photosynthese und Stoffwechsel; 04 Genetik (DNA, Gen, Vererbung); 05 Ökosysteme; 06 Kräfte und
Bewegung (Kraft, Geschwindigkeit, Newton); 07 Energie und Energieformen; 08 Chemische Reaktionen
(Sauerstoff, Verbrennung, pH-Wert); 09 Licht und Optik (Reflexion, Brechung); 10 Erde und Weltall
(Plattentektonik, Gravitation). Native anchors: Zelle, Photosynthese, DNA, Kraft, Geschwindigkeit,
pH-Wert, Brechung.

### history-dates-figures-de (73 basic / 32 cloze / 0 type_in)
Western canon + German anchors: Karl der Große (800/814), Kreuzzüge, Reformation (1517 Luther,
Augsburger Religionsfrieden 1555), Dreißigjähriger Krieg (1648 Westfälischer Frieden), Französische
Revolution (1789), Industrielle Revolution, 1848 Revolution, 1871 Reichsgründung, Erster Weltkrieg
(1914-1918), Weimarer Republik (1919), Nationalsozialismus und Zweiter Weltkrieg (1933/1939-1945),
Teilung (1949, Mauerbau 1961), Wiedervereinigung (1989/1990). Cloze cards delete the date/figure;
basic cards use natural stems (Was bedeutet „X“? per the locale qfront stem, Wer war …?, Was war …?).
No type_in (locale date formats).

### english-vocabulary-de (47 basic / 32 cloze / 26 type_in)
EFL deck for German speakers: English word → German gloss. Patterns (from he reference): basic = front
English word, back «gloss + English example sentence», extra German usage note; cloze = front «gloss +
English sentence with {{c1::word}}», back = word, extra German note; type_in = front «Schreiben Sie
das englische Wort: …» (German cue), back = English word. No direction column (build_tags allows only
en-he/he-en). School/daily-life word families mirroring the es deck scope; glossary-key words
(teacher, student, class*, question, practice…) use the glossary.csv de target (Lehrkraft, Schüler,
Klasse, Frage, Üben) inside the card text so the glossary alignment check passes.

## Quality bars applied

BAR 1 (specs/qa/bar1-rules.yaml) + compiler limits (content/seed/generate_decks.py): cloze rendered
text ≤120 chars (hard), type_in front ≤120 (hard), basic front ≤140 / back ≤320 / back ≥20 (warn
bounds), one {{c1:: per cloze, no markup/IPA slashes, no cross-card references, fronts unique,
difficulty=core uniform (matches reference; spread warn is baseline), tags subject::/grade::/unit::
locale-invariant. Census asserted post-compile against the manifest.

## Deck registry

New deck_ids: math 1699412247030, science 1699412247031, history 1699412247032,
english-vocabulary 1699412247033 (030-033 block; 017/018 reserved for the pt/it collision fix).
Titles keep the untranslated «AnkiTov Pilot — » prefix (R1).
