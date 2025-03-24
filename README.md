# Default Templates

<img align="center" width="300" src="default_templates.png" />

Followed the [$1 Unistroke Recognizer](https://depts.washington.edu/acelab/proj/dollar/index.html) documentation for this<br>

Being the simplest one of all the $ stroke algorithms this one does have his flaws, the main one being that the result really depends on how you draw the gesture, a circle drawn anticlockwise from the right would register as a circle due to it being already present in the default template and thanks due to the nearest path-distance algorithm, but a circle drawn clockwise from the right would usually be recognized as not a circle due to the path distance being closer to a shape such as a rectangle.<br>
*TL;DR A gesture does not account for all the different ways it can be drawn; i.e all the permutations.*

To fix that I have added custom gesture addition, if the shape such as a clockwise circle is misrecognized simply adding it as another gesture would now make it easily recognizable. Why did I not add it in the default templates? I did not want to.<br>

Also this algorithm does not work for horizontal/vertical lines as the scaling causes some issues, I could manually check for collinearity of the points and just call it a line but that is not fun right?<br>

*PS: I am pretty sure the milliseconds shower in the web build is inaccurate; I am not really sure why but I think it is related to wasm-unknown-unknown not having access to the standard library*

I will also code the more accurate [$P Point-Cloud Recognizer](https://depts.washington.edu/acelab/proj/dollar/pdollar.html) which enables multi-stroke gestures and is way more accurate also.