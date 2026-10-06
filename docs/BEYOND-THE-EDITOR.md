# Beyond the editor

The Mi Band App Editor makes a page of buttons. Each button sends one signal, and Mi Band Bridge runs the actions you set for it. That covers most uses: slides, media keys, shortcuts and scripts.

It cannot make:

- a button that switches between two states, such as Play and Pause
- a button that counts presses
- a screen that shows something your computer sends, such as a status

The band can do all three, in a band app written by hand. An AI coding agent can write it for you:

1. Make a first version in the editor, with the buttons and signals you want.
2. Give your agent the app's folder and the file `AGENTS.md` from this repository. Say what to change, for example "Make button 2 switch between Play and Pause."
3. Build and install the app as `AGENTS.md` says. Then set actions for its new signals in Mi Band Bridge.

A status from your computer also needs a small change to Mi Band Bridge itself, and `AGENTS.md` says where to make it.

You use your own agent and your own account with it. This project does not provide one.
