/* Ambar OS installer slideshow. */
import QtQuick 2.15
import calamares.slideshow 1.0

Presentation {
    id: presentation
    function nextSlide() { presentation.goToNextSlide(); }
    Timer { id: advance; interval: 7000; running: presentation.activatedInCalamares; repeat: true; onTriggered: nextSlide() }
    Repeater {
        model: ["slide1.png", "slide2.png", "slide3.png"]
        Slide {
            Image { source: modelData; anchors.fill: parent; fillMode: Image.PreserveAspectFit; smooth: true }
        }
    }
    function onActivate() { presentation.currentSlide = 0; }
    function onLeave() {}
}
