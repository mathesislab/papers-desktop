# Citation styles

CSL styles and the en-US locale from the
[Citation Style Language](https://citationstyles.org) project, unchanged:

- styles: <https://github.com/citation-style-language/styles>
- locale: <https://github.com/citation-style-language/locales>

They are licensed under the
[Creative Commons Attribution-ShareAlike 3.0 License](http://creativecommons.org/licenses/by-sa/3.0/);
the authors of each style are listed in its `<info>` block.

To add a style to the app, put its `.csl` file here and list it in
`src/styles.rs`. Any other style works without that: set `citation_style`
to the path of its `.csl` file in the settings.
