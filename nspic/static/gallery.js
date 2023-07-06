const AllIndicatorLists = Array.from(document.querySelectorAll("ul.ScrollIndicators"));

AllIndicatorLists.forEach(function(indi_list) {
    const Post = indi_list.parentElement;
    const ImageList = Post.querySelector("ul.ImageList");
    const Images = Array.from(Post.querySelectorAll("ul.ImageList > li"));
    const Indicators = Array.from(indi_list.querySelectorAll("li.ScrollIndicator"));

    const Observer = new IntersectionObserver(onIntersectionObserved, {
        root: ImageList,
        threshold: 0.6
    });

    function onIntersectionObserved(entries)
    {
        entries.forEach(entry => {
            // On page load, firefox claims item with index 1 isIntersecting,
            // while intersectionRatio is 0
            if (entry.isIntersecting && entry.intersectionRatio >= 0.6) {
                const IntersectingIndex = Images.indexOf(entry.target);
                activateIndicator(IntersectingIndex);
            }
        });
    }

    function activateIndicator(index)
    {
        Indicators.forEach((indicator, i) => {
            indicator.classList.toggle("Active", i === index);
        });
    }

    Images.forEach(item => {
        Observer.observe(item);
    });
});
